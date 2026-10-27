use super::*;

// Exercise backend selection/argument policy independently of the early window
// handoff. This records the fully prepared launch without a real UI or model.
fn execute(plan: Plan, operations: &mut impl Operations, working: &Path) -> crate::Result<Value> {
    let chat = plan.request.chat_path.clone();
    let models_only = plan.request.speech_models_only;
    let prepared = prepare_backend(plan, operations, working)?;
    if models_only {
        return Ok(prepared);
    }
    let arguments = prepared["Arguments"]
        .as_array()
        .ok_or("Missing prepared arguments.")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(OsString::from)
                .ok_or_else(|| "Invalid argument.".to_owned())
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let chat = chat.unwrap_or_else(|| working.parent().unwrap_or(working).join("built-chat.exe"));
    let code = operations.launch(&chat, &arguments, working)?;
    if code != 0 {
        return Err(format!("Chat stopped with exit code {code}."));
    }
    Ok(prepared)
}

#[derive(Debug, Eq, PartialEq)]
enum Event {
    Models(String, bool),
    Apps(Vec<String>, bool),
    Native(String, bool),
    Keys(PathBuf),
    Launch(PathBuf, Vec<OsString>, PathBuf),
}

struct Recorder {
    root: PathBuf,
    events: Vec<Event>,
    exit_code: u32,
    fail_native: bool,
}

impl Recorder {
    const fn new(root: PathBuf) -> Self {
        Self {
            root,
            events: Vec::new(),
            exit_code: 0,
            fail_native: false,
        }
    }

    fn launch(&self) -> crate::Result<(&Path, &[OsString], &Path)> {
        self.events
            .iter()
            .find_map(|event| match event {
                Event::Launch(path, arguments, working) => {
                    Some((path.as_path(), arguments.as_slice(), working.as_path()))
                }
                _ => None,
            })
            .ok_or_else(|| "No recorded launch.".into())
    }
}

impl Operations for Recorder {
    fn models(&mut self, request: &Request, model: &str, _working: &Path) -> crate::Result<Value> {
        self.events
            .push(Event::Models(model.into(), request.speech_models_only));
        Ok(json!({
            "CanaryModelPath": self.root.join("canary.gguf"),
            "TtsModelPath": self.root.join("talker.gguf"),
            "TtsCodecPath": self.root.join("codec.gguf"),
            "TtsVoiceModelPath": self.root.join("voice.gguf"),
            "TotalBytes": 1,
        }))
    }

    fn apps(
        &mut self,
        request: &Request,
        packages: &[&str],
        _working: &Path,
    ) -> crate::Result<Value> {
        self.events.push(Event::Apps(
            packages.iter().map(ToString::to_string).collect(),
            request.force_build,
        ));
        Ok(json!({
            "ChatPath": self.root.join("built-chat.exe"),
            "BrokerPath": self.root.join("built-broker.exe"),
        }))
    }

    fn native(&mut self, request: &Request, model: &str, _working: &Path) -> crate::Result<Value> {
        self.events
            .push(Event::Native(model.into(), request.force_build));
        if self.fail_native {
            return Err("Deliberate native build failure.".into());
        }
        Ok(json!({
            "AsrPath": self.root.join("built-asr.exe"),
            "TtsPath": self.root.join("built-tts.exe"),
        }))
    }

    fn keys(&mut self, parent: &Path) -> crate::Result<PathBuf> {
        self.events.push(Event::Keys(parent.into()));
        Ok(parent.join("frontend.key"))
    }

    fn launch(
        &mut self,
        executable: &Path,
        arguments: &[OsString],
        working: &Path,
    ) -> crate::Result<u32> {
        self.events.push(Event::Launch(
            executable.into(),
            arguments.to_vec(),
            working.into(),
        ));
        Ok(self.exit_code)
    }
}

struct Fixture {
    root: PathBuf,
    working: PathBuf,
    request: Value,
}

fn fixture(name: &str) -> crate::Result<Fixture> {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "SwiftBonsai-ChatLaunchTests-{}-{time}-{name}",
        std::process::id()
    ));
    let working = root.join("session");
    for directory in [
        &root,
        &root.join("data"),
        &root.join("repository"),
        &working,
    ] {
        fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    }
    for name in [
        "chat.exe",
        "built-chat.exe",
        "broker.exe",
        "built-broker.exe",
        "asr.exe",
        "tts.exe",
        "external.key",
    ] {
        fs::write(root.join(name), b"fixture").map_err(|error| error.to_string())?;
    }
    let request = json!({
        "DataDirectory": root.join("data"), "RepositoryRoot": root.join("repository"),
        "WorkingDirectory": working, "ChatPath": root.join("chat.exe"),
        "BrokerPath": root.join("broker.exe"),
    });
    Ok(Fixture {
        root,
        working,
        request,
    })
}

fn plan(value: Value) -> crate::Result<Plan> {
    prepare(
        serde_json::from_value(value).map_err(|error| error.to_string())?,
        true,
    )
}

fn value_after(arguments: &[OsString], option: &str) -> crate::Result<OsString> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == option)
        .map(|pair| pair[1].clone())
        .ok_or_else(|| format!("Missing launch option {option}."))
}

#[test]
fn native_window_handoff_precedes_backend_downloads_builds_and_keys() -> crate::Result<()> {
    for prebuilt_chat in [false, true] {
        let mut fixture = fixture(if prebuilt_chat {
            "early-prebuilt"
        } else {
            "early-build"
        })?;
        if !prebuilt_chat {
            fixture
                .request
                .as_object_mut()
                .ok_or("Expected request.")?
                .remove("ChatPath");
        }
        fixture.request["ForceBuild"] = json!(true);
        let plan = plan(fixture.request)?;
        let mut recorder = Recorder::new(fixture.root.clone());
        let tool = fixture.root.join("maintenance.exe");
        launch_pending(plan.request, &mut recorder, &fixture.working, &tool)?;
        assert_eq!(recorder.events.len(), if prebuilt_chat { 1 } else { 2 });
        if !prebuilt_chat {
            assert_eq!(
                recorder.events[0],
                Event::Apps(vec!["bonsai-chat".into()], true)
            );
        }
        let (_, arguments, _) = recorder.launch()?;
        assert_eq!(value_after(arguments, "--startup-tool")?, tool);
        assert_eq!(
            value_after(arguments, "--startup-data-directory")?,
            fixture.root.join("data")
        );
        let request_path = PathBuf::from(value_after(arguments, "--startup-request")?);
        let request: Request =
            serde_json::from_slice(&fs::read(request_path).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        assert!(request.chat_path.is_some());
        assert!(request.force_build);
        assert_eq!(
            request.working_directory.as_deref(),
            Some(fixture.working.as_path())
        );
        let deferred = prepare(request, true)?;
        assert!(!deferred.app_packages.contains(&"bonsai-chat"));
        assert_eq!(deferred.native_selection, Some("All"));
    }
    Ok(())
}

#[test]
fn deferred_preparation_never_rebuilds_running_chat_and_keeps_backend_policy() -> crate::Result<()>
{
    let mut fixture = fixture("deferred-broker")?;
    fixture
        .request
        .as_object_mut()
        .ok_or("Expected request.")?
        .remove("BrokerPath");
    fixture.request["ForceBuild"] = json!(true);
    let mut recorder = Recorder::new(fixture.root);
    let prepared = prepare_backend(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert!(
        recorder
            .events
            .contains(&Event::Apps(vec!["bonsai-broker".into()], true))
    );
    assert!(
        recorder
            .events
            .iter()
            .all(|event| !matches!(event, Event::Launch(..)))
    );
    assert!(
        prepared["Arguments"]
            .as_array()
            .ok_or("Missing arguments.")?
            .iter()
            .any(|arg| arg == "--broker-path")
    );
    assert_eq!(
        recorder
            .events
            .iter()
            .filter(|event| matches!(event, Event::Keys(_)))
            .count(),
        2
    );
    Ok(())
}

#[test]
fn early_window_failure_retains_nonzero_exit_and_skips_backend_work() -> crate::Result<()> {
    let fixture = fixture("early-failure")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    recorder.exit_code = 7;
    let error = launch_pending(
        plan(fixture.request)?.request,
        &mut recorder,
        &fixture.working,
        &fixture.root.join("maintenance.exe"),
    )
    .err()
    .ok_or("Expected native launch failure.")?;
    assert!(error.contains("exit code 7"));
    assert_eq!(recorder.events.len(), 1);
    assert!(matches!(recorder.events[0], Event::Launch(..)));
    Ok(())
}

#[test]
fn ordinary_startup_only_prepares_owned_services_and_creates_independent_keys() -> crate::Result<()>
{
    let fixture = fixture("owned")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert_eq!(
        &recorder.events[..4],
        &[
            Event::Models("All".into(), false),
            Event::Native("All".into(), false),
            Event::Keys(fixture.working.join("canary-credentials")),
            Event::Keys(fixture.working.join("tts-credentials")),
        ]
    );
    let (executable, arguments, working) = recorder.launch()?;
    assert_eq!(executable, fixture.root.join("chat.exe"));
    assert_eq!(working, fixture.working);
    assert_ne!(
        value_after(arguments, "--canary-key-file")?,
        value_after(arguments, "--tts-key-file")?
    );
    assert!(!arguments.iter().any(|value| value == "--use-repacked-ptq1"));
    assert_eq!(value_after(arguments, "--tts-quantization")?, "Q8_0");
    assert_eq!(
        value_after(arguments, "--tts-voice-model")?,
        fixture.root.join("voice.gguf").as_os_str()
    );
    Ok(())
}

#[test]
fn explicit_speech_files_and_text_only_never_build_or_download_speech() -> crate::Result<()> {
    for text_only in [false, true] {
        let mut fixture = fixture(if text_only { "text" } else { "external" })?;
        if text_only {
            fixture.request["NoSpeechModels"] = json!(true);
        } else {
            fixture.request["CanaryKeyFile"] = json!(fixture.root.join("external.key"));
            fixture.request["TtsKeyFile"] = json!(fixture.root.join("external.key"));
        }
        let mut recorder = Recorder::new(fixture.root.clone());
        execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        assert_eq!(recorder.events.len(), 1);
        let (_, arguments, _) = recorder.launch()?;
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--canary-executable")
        );
        assert!(
            !arguments
                .iter()
                .any(|argument| argument == "--tts-executable")
        );
        assert_eq!(
            arguments
                .iter()
                .any(|argument| argument == "--tts-key-file"),
            !text_only
        );
    }
    Ok(())
}

#[test]
fn one_owned_service_does_not_prepare_the_other_engine() -> crate::Result<()> {
    for (property, value, expected) in [
        ("NoTts", json!(true), "Canary"),
        ("CanaryKeyFile", Value::Null, "QwenTts"),
    ] {
        let mut fixture = fixture(expected)?;
        fixture.request[property] = if property == "CanaryKeyFile" {
            json!(fixture.root.join("external.key"))
        } else {
            value
        };
        let mut recorder = Recorder::new(fixture.root.clone());
        execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
        assert_eq!(recorder.events[0], Event::Models(expected.into(), false));
        assert_eq!(recorder.events[1], Event::Native(expected.into(), false));
        assert_eq!(
            recorder
                .events
                .iter()
                .filter(|event| matches!(event, Event::Keys(_)))
                .count(),
            1
        );
    }
    Ok(())
}

#[test]
fn speech_models_only_fully_verifies_without_app_builds_keys_or_launch() -> crate::Result<()> {
    let mut fixture = fixture("models-only")?;
    fixture.request["SpeechModelsOnly"] = json!(true);
    fixture.request["DataDirectory"] = json!(fixture.root.join("uncreated-data"));
    let request: Request =
        serde_json::from_value(fixture.request).map_err(|error| error.to_string())?;
    let mut recorder = Recorder::new(fixture.root.clone());
    let result = execute(prepare(request, false)?, &mut recorder, &fixture.working)?;
    assert_eq!(recorder.events, [Event::Models("All".into(), true)]);
    assert_eq!(result["TotalBytes"], 1);
    assert!(fixture.root.join("uncreated-data").is_dir());
    Ok(())
}

#[test]
fn prebuilt_paths_skip_builds_and_all_profile_choices_are_forwarded() -> crate::Result<()> {
    let mut fixture = fixture("forwarded")?;
    fixture.request["AsrPath"] = json!(fixture.root.join("asr.exe"));
    fixture.request["TtsPath"] = json!(fixture.root.join("tts.exe"));
    fixture.request["TtsQuantization"] = json!("q4_k_m");
    fixture.request["TtsThreads"] = json!(10);
    fixture.request["UseRepackedPtq1"] = json!(true);
    fixture.request["NoAbliterateLora"] = json!(true);
    fixture.request["FrontendPort"] = json!(9001);
    fixture.request["BackendPort"] = json!(9002);
    fixture.request["CanaryPort"] = json!(9003);
    fixture.request["TtsPort"] = json!(9004);
    let mut recorder = Recorder::new(fixture.root.clone());
    execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert!(
        !recorder
            .events
            .iter()
            .any(|event| matches!(event, Event::Apps(..) | Event::Native(..)))
    );
    let (_, arguments, _) = recorder.launch()?;
    for (option, expected) in [
        ("--tts-quantization", "Q4_K_M"),
        ("--tts-threads", "10"),
        ("--frontend-port", "9001"),
        ("--backend-port", "9002"),
        ("--canary-port", "9003"),
        ("--tts-port", "9004"),
    ] {
        assert_eq!(value_after(arguments, option)?, expected);
    }
    assert!(arguments.iter().any(|value| value == "--use-repacked-ptq1"));
    assert!(
        arguments
            .iter()
            .any(|value| value == "--no-abliterate-lora")
    );
    Ok(())
}

#[test]
fn missing_app_selection_and_force_build_reach_build_owners() -> crate::Result<()> {
    let mut fixture = fixture("build")?;
    fixture
        .request
        .as_object_mut()
        .ok_or("Expected request.")?
        .remove("ChatPath");
    fixture.request["ForceBuild"] = json!(true);
    let mut recorder = Recorder::new(fixture.root.clone());
    execute(plan(fixture.request)?, &mut recorder, &fixture.working)?;
    assert_eq!(
        recorder.events[1],
        Event::Apps(vec!["bonsai-chat".into()], true)
    );
    assert_eq!(recorder.events[2], Event::Native("All".into(), true));
    let (chat, arguments, _) = recorder.launch()?;
    assert_eq!(chat, fixture.root.join("built-chat.exe"));
    assert_eq!(
        value_after(arguments, "--broker-path")?,
        fixture.root.join("broker.exe").into_os_string()
    );
    Ok(())
}

#[test]
fn invalid_options_reject_before_provisioning() -> crate::Result<()> {
    let fixture = fixture("invalid")?;
    for patch in [
        json!({"FrontendPort": 8081}),
        json!({"FrontendPort": 80}),
        json!({"CanaryPort": 5173}),
        json!({"CanaryPort": 8767}),
        json!({"SpeechModelsOnly": true, "NoSpeechModels": true}),
        json!({"NoTts": true, "TtsQuantization": "Q4_K_M"}),
        json!({"NoSpeechModels": true, "TtsThreads": 1}),
        json!({"NoTts": true, "TtsKeyFile": fixture.root.join("external.key")}),
        json!({"NoSpeechModels": true, "AsrPath": fixture.root.join("asr.exe")}),
        json!({"TtsThreads": 257}),
        json!({"ChatPath": fixture.root.join("external.key")}),
        json!({"DataDirectory": "relative-data"}),
        json!({"DataDirectory": fixture.root.join("missing-data")}),
        json!({"CanaryKeyFile": fixture.root.join("missing-key")}),
    ] {
        let mut request = fixture.request.clone();
        for (key, value) in patch.as_object().ok_or("Expected patch.")? {
            request[key] = value.clone();
        }
        assert!(plan(request).is_err(), "Accepted invalid options: {patch}");
    }
    let mut unknown = fixture.request.clone();
    unknown["ArbitraryRuntimeArguments"] = json!("--unsafe");
    assert!(serde_json::from_value::<Request>(unknown).is_err());
    let request = serde_json::from_value(fixture.request).map_err(|error| error.to_string())?;
    assert!(prepare(request, false).is_err());
    Ok(())
}

#[test]
fn build_failure_and_nonzero_app_exit_remain_errors() -> crate::Result<()> {
    let fixture = fixture("failure")?;
    let mut recorder = Recorder::new(fixture.root.clone());
    recorder.fail_native = true;
    assert!(
        execute(
            plan(fixture.request.clone())?,
            &mut recorder,
            &fixture.working
        )
        .is_err()
    );
    assert!(recorder.launch().is_err());
    let mut recorder = Recorder::new(fixture.root);
    recorder.exit_code = 7;
    let error = execute(plan(fixture.request)?, &mut recorder, &fixture.working)
        .err()
        .ok_or("Expected app exit failure.")?;
    assert!(error.contains("exit code 7"));
    Ok(())
}
