//! Точка входа настольного приложения Reforged Medieval.
//!
//! Здесь только запуск: разобрать параметры, включить журнал, загрузить
//! данные и битву, создать цикл событий и передать управление в [`app::App`].
//! Как устроено приложение целиком: `docs/app/README.md`.

mod app;
mod camera;
mod campaign;
mod campaign_app;
mod font;
mod game;
mod hud;
mod options;
mod render;
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
    let data_dir = game::find_data_dir(options.data_dir.as_deref())?;
    log::info!("Данные: {}", data_dir.display());
    if options.campaign {
        let (view, baked, mesh) =
            campaign::CampaignView::new(&data_dir, options.seed, options.demo)?;
        let event_loop = EventLoop::new()?;
        let mut app = campaign_app::CampaignApp::new(options, view, baked, mesh);
        event_loop.run_app(&mut app)?;
        return match app.error.take() {
            Some(e) => Err(e),
            None => Ok(()),
        };
    }
    let game = game::Game::new(&data_dir, options.seed, options.demo)?;
    let event_loop = EventLoop::new()?;
    let mut app = app::App::new(options, game);
    event_loop.run_app(&mut app)?;
    match app.error.take() {
        Some(e) => Err(e),
        None => Ok(()),
    }
}
