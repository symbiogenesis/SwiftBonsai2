@{
    # Keep native-linking prerequisites explicit instead of enabling every
    # workspace feature in each lane. '*' means all features for one package.
    Packages = @(
        @{ Name = 'bonsai-ternary'; Manifest = 'tools/ternary/Cargo.toml'; Group = 'Runtime'; Features = @('*') }
        @{ Name = 'bonsai-codec-probe'; Manifest = 'tools/codec-probe/Cargo.toml'; Group = 'Runtime'; Features = @('*') }
        @{ Name = 'bonsai-gguf'; Manifest = 'tools/gguf/Cargo.toml'; Group = 'Runtime'; Features = @('*') }
        @{ Name = 'bonsai-prism'; Manifest = 'tools/prism/Cargo.toml'; Group = 'Runtime'; Features = @(); CompatibilityCompileFeatures = @('engine-loader'); NativeFeatures = @('*') }
        @{ Name = 'bonsai-prism-runtime'; Manifest = 'src/prism-runtime/Cargo.toml'; Group = 'Runtime'; Features = @('*') }
        @{ Name = 'bonsai-rust-trial'; Manifest = 'tools/rust-trial/Cargo.toml'; Group = 'Runtime'; Features = @('*') }
        @{ Name = 'bonsai-windows-process'; Manifest = 'src/windows-process/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-windows-credentials'; Manifest = 'src/windows-credentials/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-broker'; Manifest = 'src/broker/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-artifacts'; Manifest = 'src/artifacts/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-cpu-policy'; Manifest = 'src/cpu-policy/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-tools'; Manifest = 'tools/bonsai/Cargo.toml'; Group = 'Broker'; Features = @('*') }
    )
}
