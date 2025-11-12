use std::sync::{Arc, RwLock};

pub fn install_hook(panic_message: Arc<RwLock<Option<String>>>) {
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        if thread.name() == Some("tokio-runtime-worker") {
            let payload = match info.payload().downcast_ref::<&'static str>() {
                Some(s) => *s,
                None => match info.payload().downcast_ref::<String>() {
                    Some(s) => &**s,
                    None => "Box<Any>",
                },
            };

            let backtrace = backtrace::Backtrace::new();

            let message = match info.location() {
                Some(location) => {
                    format!(
                        "Backend panicked at {}:{}:{}\n{}\n{:?}",
                        location.file(),
                        location.line(),
                        location.column(),
                        payload,
                        PrettyBacktrace(backtrace)
                    )
                }
                None => format!(
                    "Backend panicked\n{}\n{:?}",
                    payload,
                    PrettyBacktrace(backtrace)
                ),
            };

            eprintln!("{}", message);
            *panic_message.write().unwrap() = Some(message);
        } else {
            (old_hook)(info);
        }
    }));
}

struct PrettyBacktrace(backtrace::Backtrace);

impl std::fmt::Debug for PrettyBacktrace {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cwd = std::env::current_dir();
        let mut print_path =
            move |fmt: &mut std::fmt::Formatter<'_>, path: backtrace::BytesOrWideString<'_>| {
                let path = path.into_path_buf();
                if let Ok(cwd) = &cwd && let Ok(suffix) = path.strip_prefix(cwd) {
                    return std::fmt::Display::fmt(&suffix.display(), fmt);
                }
                std::fmt::Display::fmt(&path.display(), fmt)
            };

        let mut f = backtrace::BacktraceFmt::new(fmt, backtrace::PrintFmt::Short, &mut print_path);
        f.add_context()?;
        let frames = self.0.frames();
        let ignore_start = &[
            "backtrace::backtrace::trace",
            "backtrace::capture::Backtrace::create",
            "backtrace::capture::Backtrace::new",
            "pandora_launcher::panic::install_hook::{{closure}}",
            "__rustc::rust_begin_unwind",
        ];
        let mut start = 0;
        for (index, frame) in frames.iter().enumerate() {
            for symbol in frame.symbols() {
                if let Some(name) = symbol.name() {
                    let name_str = format!("{name:#}");
                    if ignore_start.contains(&name_str.as_str()) {
                        start = index;
                    }
                }
            }
        }
        for frame in &frames[start..] {
            f.frame().backtrace_frame(frame)?;
        }
        f.finish()?;
        Ok(())
    }
}
