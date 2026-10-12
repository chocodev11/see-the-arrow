//! `--say` through SAPI, which Windows always has. Voices come from the classic list and the
//! OneCore list, where most languages (Vietnamese "Microsoft An" among them) live.

use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectTokenCategory, ISpVoice, SPCAT_VOICES, SPF_ASYNC,
    SPF_PURGEBEFORESPEAK, SpObjectTokenCategory, SpVoice,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
};
use windows::core::{HSTRING, PCWSTR, w};

const ONECORE_VOICES: PCWSTR = w!(r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech_OneCore\Voices");

/// Speaks in the background; dropping it stops the voice.
pub struct Speech {
    voice: ISpVoice,
}

impl Speech {
    pub fn start(text: &str, voice: Option<&str>) -> Option<Self> {
        let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        let speaker: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL) }.ok()?;
        if let Some(token) = voice.and_then(find_voice) {
            let _ = unsafe { speaker.SetVoice(&token) };
        }
        let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
        unsafe { speaker.Speak(&HSTRING::from(text), flags, None) }.ok()?;
        Some(Self { voice: speaker })
    }
}

impl Drop for Speech {
    fn drop(&mut self) {
        let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
        let _ = unsafe { self.voice.Speak(w!(""), flags, None) };
    }
}

/// The first installed voice whose description contains `name`, case-insensitive.
fn find_voice(name: &str) -> Option<ISpObjectToken> {
    let needle = name.to_lowercase();
    for category in [SPCAT_VOICES, ONECORE_VOICES] {
        let Ok(list) = (unsafe {
            CoCreateInstance::<_, ISpObjectTokenCategory>(&SpObjectTokenCategory, None, CLSCTX_ALL)
        }) else {
            continue;
        };
        if unsafe { list.SetId(category, false) }.is_err() {
            continue;
        }
        let Ok(tokens) = (unsafe { list.EnumTokens(PCWSTR::null(), PCWSTR::null()) }) else {
            continue;
        };
        loop {
            let mut token = None;
            let mut fetched = 0;
            if unsafe { tokens.Next(1, &mut token, Some(&mut fetched)) }.is_err() || fetched == 0 {
                break;
            }
            let Some(token) = token else { break };
            let Ok(description) = (unsafe { token.GetStringValue(PCWSTR::null()) }) else {
                continue;
            };
            let text = unsafe { description.to_string() }.unwrap_or_default();
            unsafe { CoTaskMemFree(Some(description.0 as _)) };
            if text.to_lowercase().contains(&needle) {
                return Some(token);
            }
        }
    }
    None
}
