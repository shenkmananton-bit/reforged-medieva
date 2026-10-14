//! Параметры запуска из командной строки.
//!
//! Пример: `reforged-medieval --tick-rate 30 --no-vsync --frames 300`.
//! Полный список: `reforged-medieval --help` или `docs/how-to-run.md`.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

pub const HELP: &str = "\
Reforged Medieval

Использование: reforged-medieval [параметры]

  --tick-rate <N>   частота симуляции, тиков в секунду (по умолчанию 20)
  --no-vsync        не ждать вертикальную синхронизацию (больше FPS)
  --frames <N>      закрыться после N кадров (для автоматических проверок)
  --width <N>       ширина окна (по умолчанию 1280)
  --height <N>      высота окна (по умолчанию 720)
  --data <папка>    папка с данными игры (по умолчанию ищется data/ рядом)
  --seed <N>        зерно случайности битвы (по умолчанию 1)
  --demo            обе армии под управлением ИИ (смотреть бой со стороны)
  --campaign        открыть карту кампании вместо битвы (--seed меняет карту)
  -h, --help        показать эту справку

Переменные окружения:
  RUST_LOG=debug    подробный журнал
  RM_VALIDATION=1   включить слои проверки Vulkan (нужен Vulkan SDK)
";

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub tick_rate: u32,
    pub vsync: bool,
    pub exit_after_frames: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub data_dir: Option<PathBuf>,
    pub seed: u64,
    pub demo: bool,
    pub campaign: bool,
    pub show_help: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            tick_rate: 20,
            vsync: true,
            exit_after_frames: None,
            width: 1280,
            height: 720,
            data_dir: None,
            seed: 1,
            demo: false,
            campaign: false,
            show_help: false,
        }
    }
}

impl Options {
    /// Разбирает аргументы (без имени программы).
    pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Self> {
        let mut opts = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| {
                args.next()
                    .with_context(|| format!("после {name} нужно значение"))
            };
            match arg.as_str() {
                "--tick-rate" => {
                    opts.tick_rate = parse_positive("--tick-rate", &value("--tick-rate")?)?
                }
                "--no-vsync" => opts.vsync = false,
                "--frames" => {
                    opts.exit_after_frames =
                        Some(parse_positive("--frames", &value("--frames")?)?.into())
                }
                "--width" => opts.width = parse_positive("--width", &value("--width")?)?,
                "--height" => opts.height = parse_positive("--height", &value("--height")?)?,
                "--data" => opts.data_dir = Some(PathBuf::from(value("--data")?)),
                "--seed" => {
                    let raw = value("--seed")?;
                    opts.seed = raw
                        .parse()
                        .with_context(|| format!("--seed: «{raw}» не число"))?;
                }
                "--demo" => opts.demo = true,
                "--campaign" => opts.campaign = true,
                "-h" | "--help" => opts.show_help = true,
                other => bail!("неизвестный параметр {other}; см. --help"),
            }
        }
        Ok(opts)
    }
}

fn parse_positive(name: &str, raw: &str) -> Result<u32> {
    let v: u32 = raw
        .parse()
        .with_context(|| format!("{name}: «{raw}» не число"))?;
    if v == 0 {
        bail!("{name} должно быть больше нуля");
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &[&str]) -> Result<Options> {
        Options::parse(s.iter().map(|s| s.to_string()))
    }

    #[test]
    fn defaults() {
        assert_eq!(parse(&[]).unwrap(), Options::default());
    }

    #[test]
    fn all_flags() {
        let o = parse(&[
            "--tick-rate",
            "30",
            "--no-vsync",
            "--frames",
            "5",
            "--width",
            "800",
            "--height",
            "600",
            "--data",
            "mods/x",
            "--seed",
            "42",
            "--demo",
            "--campaign",
        ])
        .unwrap();
        assert!(o.campaign);
        assert_eq!(o.data_dir, Some(PathBuf::from("mods/x")));
        assert_eq!(o.seed, 42);
        assert!(o.demo);
        assert_eq!(o.tick_rate, 30);
        assert!(!o.vsync);
        assert_eq!(o.exit_after_frames, Some(5));
        assert_eq!((o.width, o.height), (800, 600));
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(&["--tick-rate", "0"]).is_err());
        assert!(parse(&["--frames"]).is_err());
        assert!(parse(&["--frames", "abc"]).is_err());
        assert!(parse(&["--wat"]).is_err());
    }
}
