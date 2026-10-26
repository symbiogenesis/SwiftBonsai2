@{
    # Runtime implementation packages live in the prism-inference repository.
    # This registry owns only SwiftBonsai packages; '*' means all features for one package.
    Packages = @(
        @{ Name = 'bonsai-windows-process'; Manifest = 'src/windows-process/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-windows-credentials'; Manifest = 'src/windows-credentials/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-broker'; Manifest = 'src/broker/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-chat'; Manifest = 'src/chat-ui/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-speech-asr'; Manifest = 'src/speech-asr/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-speech-tts'; Manifest = 'src/speech-tts/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-speech-service'; Manifest = 'src/speech-service/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-audio'; Manifest = 'src/audio/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-artifacts'; Manifest = 'src/artifacts/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-cpu-policy'; Manifest = 'src/cpu-policy/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-tools'; Manifest = 'tools/bonsai/Cargo.toml'; Group = 'Broker'; Features = @('*') }
        @{ Name = 'bonsai-ci'; Manifest = 'tools/ci/Cargo.toml'; Group = 'Broker'; Features = @('*') }
    )
}
