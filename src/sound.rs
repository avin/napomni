//! One process-wide built-in waveform; the desktop reconciles aggregate toast intent.
use anyhow::{Result, ensure};
use windows::{
    Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FLAGS, SND_LOOP, SND_MEMORY, SND_NODEFAULT},
    core::PCWSTR,
};

// SND_MEMORY ignores the string interpretation of PCWSTR. Keep the RIFF bytes
// aligned and alive for the entire asynchronous loop (including application exit).
#[repr(align(4))]
struct Wave([u8; include_bytes!("../assets/reminder.wav").len()]);
static WAVE: Wave = Wave(*include_bytes!("../assets/reminder.wav"));

#[derive(Default)]
pub struct NativeSignal {
    active: bool,
}
impl NativeSignal {
    /// Whether Windows accepted the current start intent, not an audibility meter.
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn reconcile(&mut self, active: bool) -> Result<()> {
        if active == self.active {
            return Ok(());
        }
        let success = unsafe {
            if active {
                PlaySoundW(
                    PCWSTR(WAVE.0.as_ptr().cast()),
                    None,
                    SND_MEMORY | SND_ASYNC | SND_LOOP | SND_NODEFAULT,
                )
            } else {
                PlaySoundW(PCWSTR::null(), None, SND_FLAGS(0))
            }
        };
        ensure!(
            success.as_bool(),
            if active {
                "Windows не удалось запустить встроенный сигнал"
            } else {
                "Windows не удалось остановить сигнал"
            }
        );
        self.active = active;
        Ok(())
    }
}
impl Drop for NativeSignal {
    fn drop(&mut self) {
        let _ = self.reconcile(false);
    }
}
