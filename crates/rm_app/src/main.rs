//! Точка входа настольного приложения Reforged Medieval.
//!
//! Здесь только запуск: разобрать параметры, включить журнал,
//! создать цикл событий и передать управление в [`app::App`].
//! Как устроено приложение целиком: `docs/app/README.md`.

mod app;
mod options;
mod render;
mod sim;
mod timing;

use std::process::ExitCode;

use winit::event_loop::EventLoop;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = match options::Options::parse(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("Ошибка: {e:#}\n\n{}", options::HELP);
            return ExitCode::from(2);
        }
    };
    if options.show_help {
        print!("{}", options::HELP);
        return ExitCode::SUCCESS;
    }

    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            log::error!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(options: options::Options) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = app::App::new(options);
    event_loop.run_app(&mut app)?;
    match app.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
