//! Bounded read-only Vulkan queries. The caller owns the driver-call deadline.

use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt as _;
use std::path::PathBuf;

use ash::{Entry, vk};

use crate::{VulkanAdapter, VulkanInventory, VulkanQueryFailure};

const MAX_ADAPTERS: usize = 16;
const MAX_QUEUES: usize = 64;
const MAX_EXTENSIONS: usize = 1024;

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "unsupported Vulkan inventory")
}

fn failure(
    stage: &'static str,
    vulkan_result: Option<i32>,
    loader_api_version: Option<u32>,
    application_api_version: Option<u32>,
    kind: io::ErrorKind,
) -> io::Error {
    io::Error::new(
        kind,
        VulkanQueryFailure {
            stage,
            vulkan_result,
            loader_api_version,
            application_api_version,
        },
    )
}

fn check(result: vk::Result) -> io::Result<()> {
    if result == vk::Result::SUCCESS {
        Ok(())
    } else {
        Err(failure(
            "device-query",
            Some(result.as_raw()),
            None,
            None,
            io::ErrorKind::Other,
        ))
    }
}

fn text(bytes: &[std::ffi::c_char]) -> io::Result<String> {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(invalid)?;
    let bytes: Vec<_> = bytes[..end]
        .iter()
        .map(|byte| byte.to_ne_bytes()[0])
        .collect();
    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
    if text.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(text)
}

fn loader() -> io::Result<Entry> {
    let mut buffer = vec![0_u16; 32768];
    // SAFETY: The writable buffer has the advertised size and remains live for
    // the call. GetSystemDirectoryW retains no pointers.
    let count = unsafe {
        windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW(
            buffer.as_mut_ptr(),
            32768,
        )
    };
    let count = usize::try_from(count).map_err(|_| invalid())?;
    if count == 0 || count >= buffer.len() {
        return Err(io::Error::last_os_error());
    }
    let path = PathBuf::from(OsString::from_wide(&buffer[..count])).join("vulkan-1.dll");
    // SAFETY: An absolute Windows system-directory loader is selected, without
    // application/current-directory search. Vulkan overrides reject before this
    // call. Installed graphics loader/ICDs remain the operator's trust boundary.
    // Entry remains alive until the instance and every query have been dropped.
    unsafe { Entry::load_from(path) }
        .map_err(|_| io::Error::other("system Vulkan loader unavailable"))
}

struct Instance(ash::Instance);

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the instance, creates no children,
        // and is dropped before the Entry providing its function pointers.
        unsafe { self.0.destroy_instance(None) };
    }
}

const fn device_type(kind: vk::PhysicalDeviceType) -> (&'static str, bool) {
    match kind {
        vk::PhysicalDeviceType::INTEGRATED_GPU => ("integrated-gpu", true),
        vk::PhysicalDeviceType::DISCRETE_GPU => ("discrete-gpu", true),
        vk::PhysicalDeviceType::VIRTUAL_GPU => ("virtual-gpu", false),
        vk::PhysicalDeviceType::CPU => ("cpu", false),
        _ => ("other", false),
    }
}

#[derive(Default)]
struct Features {
    float16: Option<bool>,
    integer_dot: Option<bool>,
    subgroup_size: Option<u32>,
    subgroup_operations: Option<u32>,
    subgroup_stages: Option<u32>,
}

fn feature_structures(
    application_api: u32,
    device_api: u32,
    extensions: &[String],
) -> Option<(bool, bool)> {
    let effective_api = application_api.min(device_api);
    (effective_api >= vk::API_VERSION_1_1).then(|| {
        (
            effective_api >= vk::API_VERSION_1_2
                || extensions
                    .iter()
                    .any(|name| name == "VK_KHR_shader_float16_int8"),
            effective_api >= vk::API_VERSION_1_3
                || extensions
                    .iter()
                    .any(|name| name == "VK_KHR_shader_integer_dot_product"),
        )
    })
}

fn features(
    instance: &ash::Instance,
    device: vk::PhysicalDevice,
    api: u32,
    extensions: &[String],
    application_api: u32,
) -> Features {
    let Some((supports_half, supports_dot)) = feature_structures(application_api, api, extensions)
    else {
        return Features::default();
    };
    let mut half = vk::PhysicalDeviceShaderFloat16Int8Features::default();
    let mut dot = vk::PhysicalDeviceShaderIntegerDotProductFeatures::default();
    let mut features = vk::PhysicalDeviceFeatures2::default();
    if supports_half {
        features = features.push_next(&mut half);
    }
    if supports_dot {
        features = features.push_next(&mut dot);
    }
    // SAFETY: Vulkan 1.1 query with a valid live device. Only supported core
    // or advertised extension feature structures are chained and stay live.
    unsafe { instance.get_physical_device_features2(device, &mut features) };
    let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
    let mut properties2 = vk::PhysicalDeviceProperties2::default().push_next(&mut subgroup);
    // SAFETY: Both structures are supported core 1.1 structures and live
    // throughout the read-only properties query against this valid device.
    unsafe { instance.get_physical_device_properties2(device, &mut properties2) };
    Features {
        float16: supports_half.then_some(half.shader_float16 != 0),
        integer_dot: supports_dot.then_some(dot.shader_integer_dot_product != 0),
        subgroup_size: Some(subgroup.subgroup_size),
        subgroup_operations: Some(subgroup.supported_operations.as_raw()),
        subgroup_stages: Some(subgroup.supported_stages.as_raw()),
    }
}

fn adapter(
    instance: &ash::Instance,
    device: vk::PhysicalDevice,
    application_api: u32,
) -> io::Result<VulkanAdapter> {
    // SAFETY: device was enumerated from this live instance; the read-only
    // query returns an initialized value and retains no borrowed storage.
    let properties = unsafe { instance.get_physical_device_properties(device) };
    let mut extensions = vec![vk::ExtensionProperties::default(); MAX_EXTENSIONS];
    let mut count = u32::try_from(extensions.len()).map_err(|_| invalid())?;
    // SAFETY: Valid instance/device, no layer filter, initialized output storage
    // for count entries. The driver may only write that many entries.
    check(unsafe {
        (instance.fp_v1_0().enumerate_device_extension_properties)(
            device,
            std::ptr::null(),
            &raw mut count,
            extensions.as_mut_ptr(),
        )
    })?;
    let count = usize::try_from(count).map_err(|_| invalid())?;
    if count > extensions.len() {
        return Err(invalid());
    }
    extensions.truncate(count);
    let mut extensions: Vec<_> = extensions
        .iter()
        .map(|extension| text(&extension.extension_name))
        .collect::<io::Result<_>>()?;
    extensions.sort_unstable();

    let mut queues = [vk::QueueFamilyProperties::default(); MAX_QUEUES];
    let mut queue_count = 0;
    // SAFETY: Valid device and writable count; null storage requests only count.
    unsafe {
        (instance
            .fp_v1_0()
            .get_physical_device_queue_family_properties)(
            device,
            &raw mut queue_count,
            std::ptr::null_mut(),
        );
    };
    if usize::try_from(queue_count).map_err(|_| invalid())? > queues.len() {
        return Err(invalid());
    }
    // SAFETY: The bounded initialized output array accommodates the queried
    // count. Vulkan writes at most the input count and retains no pointers.
    unsafe {
        (instance
            .fp_v1_0()
            .get_physical_device_queue_family_properties)(
            device,
            &raw mut queue_count,
            queues.as_mut_ptr(),
        );
    };
    let queue_count = usize::try_from(queue_count).map_err(|_| invalid())?;
    let compute_queues = queues
        .get(..queue_count)
        .ok_or_else(invalid)?
        .iter()
        .enumerate()
        .filter(|(_, queue)| queue.queue_flags.contains(vk::QueueFlags::COMPUTE))
        .map(|(index, queue)| u32::try_from(index).map(|index| (index, queue.queue_count)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| invalid())?;

    // SAFETY: Valid device from this live instance. Returned fixed-size memory
    // properties are fully initialized by Vulkan and do not borrow driver data.
    let memory = unsafe { instance.get_physical_device_memory_properties(device) };
    let heaps = usize::try_from(memory.memory_heap_count).map_err(|_| invalid())?;
    let memory_heaps = memory
        .memory_heaps
        .get(..heaps)
        .ok_or_else(invalid)?
        .iter()
        .map(|heap| {
            (
                heap.size,
                heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL),
            )
        })
        .collect();

    let features = features(
        instance,
        device,
        properties.api_version,
        &extensions,
        application_api,
    );
    let (device_type, hardware_gpu) = device_type(properties.device_type);
    Ok(VulkanAdapter {
        name: text(&properties.device_name)?,
        vendor_id: properties.vendor_id,
        device_id: properties.device_id,
        driver_version: properties.driver_version,
        api_version: properties.api_version,
        device_type,
        hardware_gpu,
        compute_queues,
        extensions,
        memory_heaps,
        shader_float16: features.float16,
        shader_integer_dot_product: features.integer_dot,
        subgroup_size: features.subgroup_size,
        subgroup_operations: features.subgroup_operations,
        subgroup_stages: features.subgroup_stages,
    })
}

pub(super) fn query() -> io::Result<VulkanInventory> {
    if !cfg!(target_arch = "aarch64")
        || std::env::vars_os().any(|(name, _)| {
            name.to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("VK_")
        })
    {
        return Err(failure(
            "eligibility",
            None,
            None,
            None,
            io::ErrorKind::Unsupported,
        ));
    }
    let entry =
        loader().map_err(|error| failure("system-loader", None, None, None, error.kind()))?;
    // SAFETY: The Entry owns the live native Vulkan loader. Version enumeration
    // needs no Vulkan instance and ash handles a missing Vulkan 1.0 entry point.
    let loader_api_version = unsafe { entry.try_enumerate_instance_version() }
        .map_err(|error| {
            failure(
                "loader-version",
                Some(error.as_raw()),
                None,
                None,
                io::ErrorKind::Other,
            )
        })?
        .unwrap_or(vk::API_VERSION_1_0);
    let application_api = loader_api_version.min(vk::API_VERSION_1_3);
    let application = vk::ApplicationInfo::default()
        .application_name(c"bonsai-inventory")
        .api_version(application_api);
    let create = vk::InstanceCreateInfo::default().application_info(&application);
    // SAFETY: Initialized create structures have live pointers; no layers or
    // extensions are requested. The instance is destroyed before its Entry.
    let instance = Instance(
        unsafe { entry.create_instance(&create, None) }.map_err(|error| {
            failure(
                "instance-creation",
                Some(error.as_raw()),
                Some(loader_api_version),
                Some(application_api),
                io::ErrorKind::Other,
            )
        })?,
    );
    let mut devices = [vk::PhysicalDevice::null(); MAX_ADAPTERS];
    let mut count = u32::try_from(devices.len()).map_err(|_| invalid())?;
    // SAFETY: Valid instance and initialized storage for the advertised count.
    // SUCCESS is required; INCOMPLETE rejects rather than growing without bound.
    let result = unsafe {
        (instance.0.fp_v1_0().enumerate_physical_devices)(
            instance.0.handle(),
            &raw mut count,
            devices.as_mut_ptr(),
        )
    };
    if result != vk::Result::SUCCESS {
        return Err(failure(
            "physical-device-enumeration",
            Some(result.as_raw()),
            Some(loader_api_version),
            Some(application_api),
            io::ErrorKind::Other,
        ));
    }
    let count = usize::try_from(count).map_err(|_| invalid())?;
    let adapters = devices
        .get(..count)
        .ok_or_else(invalid)?
        .iter()
        .map(|device| {
            adapter(&instance.0, *device, application_api).map_err(|error| {
                let result = error
                    .get_ref()
                    .and_then(|error| error.downcast_ref::<VulkanQueryFailure>())
                    .and_then(|failure| failure.vulkan_result);
                failure(
                    "adapter-properties",
                    result,
                    Some(loader_api_version),
                    Some(application_api),
                    error.kind(),
                )
            })
        })
        .collect::<io::Result<_>>()?;
    Ok(VulkanInventory {
        loader_api_version,
        application_api_version: application_api,
        adapters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vulkan_failure_payload_preserves_raw_results_and_unknown_versions() -> io::Result<()> {
        let error = check(vk::Result::ERROR_INCOMPATIBLE_DRIVER)
            .err()
            .ok_or_else(invalid)?;
        let context = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<VulkanQueryFailure>())
            .ok_or_else(invalid)?;
        assert_eq!(context.stage, "device-query");
        assert_eq!(context.vulkan_result, Some(-9));
        assert_eq!(context.loader_api_version, None);
        assert_eq!(context.application_api_version, None);
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            error.to_string(),
            "Vulkan inventory failed at device-query (VkResult -9)"
        );
        Ok(())
    }

    #[test]
    fn vulkan_names_and_device_types_preserve_unqualified_boundaries() -> io::Result<()> {
        assert!(check(vk::Result::SUCCESS).is_ok());
        assert!(check(vk::Result::INCOMPLETE).is_err());
        assert!(check(vk::Result::ERROR_INCOMPATIBLE_DRIVER).is_err());
        assert_eq!(text(&[65, 0, 66])?, "A");
        assert!(text(&[65, 66]).is_err());
        assert!(text(&[10, 0]).is_err());
        assert_eq!(device_type(vk::PhysicalDeviceType::CPU), ("cpu", false));
        assert_eq!(
            device_type(vk::PhysicalDeviceType::VIRTUAL_GPU),
            ("virtual-gpu", false)
        );
        assert_eq!(
            device_type(vk::PhysicalDeviceType::INTEGRATED_GPU),
            ("integrated-gpu", true)
        );
        Ok(())
    }

    #[test]
    fn vulkan_feature_structures_require_application_and_device_eligibility() {
        assert_eq!(
            feature_structures(vk::API_VERSION_1_1, vk::API_VERSION_1_3, &[]),
            Some((false, false))
        );
        assert_eq!(
            feature_structures(vk::API_VERSION_1_3, vk::API_VERSION_1_1, &[]),
            Some((false, false))
        );
        assert_eq!(
            feature_structures(vk::API_VERSION_1_3, vk::API_VERSION_1_3, &[]),
            Some((true, true))
        );
        assert_eq!(
            feature_structures(vk::API_VERSION_1_0, vk::API_VERSION_1_3, &[]),
            None
        );
        assert_eq!(
            feature_structures(
                vk::API_VERSION_1_1,
                vk::API_VERSION_1_1,
                &[
                    "VK_KHR_shader_float16_int8".into(),
                    "VK_KHR_shader_integer_dot_product".into()
                ]
            ),
            Some((true, true))
        );
    }
}
