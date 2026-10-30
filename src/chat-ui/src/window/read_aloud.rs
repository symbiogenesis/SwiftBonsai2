//! Conversation preference and cancellable incremental answer playback.
use super::{State, VOICE_LIMIT, egui, speech};

impl State {
    fn valid_voice(&mut self) -> bool {
        let voice = self.voice_profile.trim();
        if !voice.is_empty() && !voice.contains('\0') && voice.chars().count() <= VOICE_LIMIT {
            return true;
        }
        "Describe the voice in Settings before reading aloud.".clone_into(&mut self.speech_status);
        self.speech_notice = true;
        self.show_settings = true;
        false
    }

    pub(super) fn toggle_streaming_read_aloud(&mut self) {
        self.speech_notice = true;
        if self.streaming_read_aloud {
            self.streaming_read_aloud = false;
            if self.streaming_active {
                self.stop_speech();
            }
        } else if !self.speech_output {
            let message = self.read_aloud_help();
            message.clone_into(&mut self.speech_status);
            self.help = Some(("Read aloud setup", message));
        } else if self.valid_voice() {
            self.streaming_read_aloud = true;
            self.streaming_suppressed = false;
            "New responses will be read aloud as they arrive.".clone_into(&mut self.speech_status);
            if self.model.busy() && !self.last_answer.is_empty() {
                self.stop_speech();
                self.streaming_suppressed = false;
                self.stream_answer(&self.last_answer.clone());
            }
        }
        self.recovery_dirty = true;
        self.record(&self.speech_status.clone());
    }

    pub(super) fn stream_answer(&mut self, text: &str) {
        if !self.model.busy()
            || !self.streaming_read_aloud
            || self.streaming_suppressed
            || !self.speech_output
            || text.is_empty()
        {
            return;
        }
        if !self.streaming_active {
            if !self.valid_voice() {
                self.streaming_suppressed = true;
                return;
            }
            self.cancel_microphone();
            if !self.speech_command(speech::Command::BeginStream {
                voice_description: self.voice_profile.trim().to_owned(),
            }) {
                return;
            }
            self.streaming_active = true;
            self.speaking = true;
            self.speaking_message = self.transcript.messages.last().map(|message| message.id);
            self.speech_notice = true;
            "Reading the response as it arrives.".clone_into(&mut self.speech_status);
        }
        self.speech_command(speech::Command::AppendStream {
            text: text.to_owned(),
        });
    }

    pub(super) fn replace_streaming_stage(&mut self) {
        if self.streaming_active {
            self.stop_speech();
            self.streaming_suppressed = false;
        }
    }

    pub(super) fn read_aloud_controls(&mut self, ui: &mut egui::Ui) {
        let mut enabled = self.streaming_read_aloud;
        if ui
            .checkbox(&mut enabled, "Read aloud as responses arrive")
            .on_hover_text("Read new responses in short chunks using your Voice profile. Speaker buttons can replay completed responses at any time.")
            .changed()
        {
            self.toggle_streaming_read_aloud();
        }
        if self.speaking && ui.button("Stop speech").clicked() {
            self.stop_speech();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        client,
        window::{ModelLifecycle, transcript::Role},
    };
    use std::{io, sync::mpsc, time::Duration};

    fn fixture() -> io::Result<(State, mpsc::Receiver<speech::Command>)> {
        let (commands, _incoming) = mpsc::channel();
        let (_events, incoming_events) = client::event_channel();
        let mut state = State::create(commands, incoming_events, None, None);
        let (session, speech_commands) = speech::Session::fixture()?;
        state.speech = Some(session);
        state.speech_output = true;
        state.preview = false;
        state.model = ModelLifecycle::Generating;
        state.transcript.push(Role::Assistant, String::new());
        Ok((state, speech_commands))
    }

    fn next(commands: &mpsc::Receiver<speech::Command>) -> io::Result<speech::Command> {
        commands
            .recv_timeout(Duration::from_secs(2))
            .map_err(io::Error::other)
    }

    #[test]
    fn streaming_read_aloud_forwards_visible_deltas_before_completion_and_finishes_once()
    -> io::Result<()> {
        let (mut state, commands) = fixture()?;
        assert!(!state.streaming_read_aloud);
        state.toggle_streaming_read_aloud();
        state.model_event(client::Event::Reasoning("Private reasoning".to_owned()));
        state.model_event(client::Event::Draft("Retained draft".to_owned()));
        state.model_event(client::Event::Text("Hello ".to_owned()));
        assert!(state.model.busy() && state.streaming_active);
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(
            matches!(next(&commands)?, speech::Command::BeginStream { voice_description } if voice_description == state.voice_profile)
        );
        assert!(
            matches!(next(&commands)?, speech::Command::AppendStream { text } if text == "Hello ")
        );
        state.model_event(client::Event::Text("world.".to_owned()));
        assert!(
            matches!(next(&commands)?, speech::Command::AppendStream { text } if text == "world.")
        );
        state.model_event(client::Event::Complete(client::Completion::TokenLimit));
        assert!(matches!(next(&commands)?, speech::Command::FinishStream));
        assert!(!state.model.busy());
        assert_eq!(state.last_answer, "Hello world.");
        Ok(())
    }

    #[test]
    fn streaming_read_aloud_manual_replay_and_stop_suppress_the_current_response() -> io::Result<()>
    {
        let (mut state, commands) = fixture()?;
        state.transcript.append("Older answer", false);
        state.transcript.remove_empty_response();
        let older = state.transcript.messages[0].id;
        state.transcript.push(Role::Assistant, String::new());
        state.toggle_streaming_read_aloud();
        state.model_event(client::Event::Text("New answer".to_owned()));
        for _ in 0..3 {
            next(&commands)?;
        }
        state.toggle_read_aloud(older);
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(matches!(next(&commands)?, speech::Command::StopSpeaking));
        assert!(
            matches!(next(&commands)?, speech::Command::Speak { text, .. } if text == "Older answer")
        );
        assert!(state.streaming_read_aloud && state.streaming_suppressed);
        assert!(!state.streaming_active);
        state.model_event(client::Event::Text(" continues".to_owned()));
        state.model_event(client::Event::Complete(client::Completion::Finished));
        assert_eq!(state.speaking_message, Some(older));
        state.toggle_read_aloud(older);
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(matches!(next(&commands)?, speech::Command::StopSpeaking));
        assert!(!state.speaking);
        assert!(commands.recv_timeout(Duration::from_millis(20)).is_err());
        Ok(())
    }

    #[test]
    fn streaming_read_aloud_stage_replacement_cancels_and_rejection_never_restarts()
    -> io::Result<()> {
        let (mut state, commands) = fixture()?;
        state.toggle_streaming_read_aloud();
        state.model_event(client::Event::Text("Draft answer".to_owned()));
        for _ in 0..3 {
            next(&commands)?;
        }
        state.model_event(client::Event::Stage("Review"));
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(matches!(next(&commands)?, speech::Command::StopSpeaking));
        state.model_event(client::Event::Text("Revised answer".to_owned()));
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(matches!(
            next(&commands)?,
            speech::Command::BeginStream { .. }
        ));
        assert!(
            matches!(next(&commands)?, speech::Command::AppendStream { text } if text == "Revised answer")
        );
        state.model_event(client::Event::Rejected {
            message: "Context limit".to_owned(),
            text: "Prompt".to_owned(),
            retry_required: false,
        });
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(matches!(next(&commands)?, speech::Command::StopSpeaking));
        state.model_event(client::Event::Text("late text".to_owned()));
        assert!(!state.streaming_active);
        assert!(commands.recv_timeout(Duration::from_millis(20)).is_err());
        Ok(())
    }

    #[test]
    fn streaming_read_aloud_toggle_off_and_current_speaker_cancel_unfinished_audio()
    -> io::Result<()> {
        let (mut state, commands) = fixture()?;
        state.toggle_streaming_read_aloud();
        state.model_event(client::Event::Text("Some text".to_owned()));
        for _ in 0..3 {
            next(&commands)?;
        }
        let current = state.transcript.messages[0].id;
        state.toggle_read_aloud(current);
        assert!(!state.speaking);
        assert!(state.streaming_read_aloud && state.streaming_suppressed);
        assert!(matches!(next(&commands)?, speech::Command::StopListening));
        assert!(matches!(next(&commands)?, speech::Command::StopSpeaking));
        state.toggle_streaming_read_aloud();
        assert!(!state.streaming_read_aloud);
        state.model_event(client::Event::Text(" continues silently".to_owned()));
        assert!(commands.recv_timeout(Duration::from_millis(20)).is_err());
        Ok(())
    }
}
