//! Command line handling.
//!
//! The application is primarily a desktop app, but the same binary has to be usable from a
//! terminal and from scripts: to open a file, to run the built-in checks, and to print the
//! version. Parsing is hand-written so there is one obvious place to read the behavior from.

use std::path::PathBuf;

/// What the user asked the process to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Start the window, optionally opening files.
    Run {
        files: Vec<PathBuf>,
        new_window: bool,
    },
    /// Open a file and write it out again elsewhere: the batch path scripts use.
    Export {
        input: PathBuf,
        output: PathBuf,
        quality: u8,
    },
    /// Install the desktop entry, the icons and a Desktop shortcut for this user.
    Install {
        desktop_shortcut: bool,
        bin_dir: Option<PathBuf>,
    },
    /// Remove what [`Command::Install`] wrote.
    Uninstall,
    /// Run the built-in checks, print a report and exit.
    SelfTest {
        verbose: bool,
    },
    /// Open the window, draw a few frames and close: the automated startup check.
    SmokeTest {
        frames: u32,
        report: Option<PathBuf>,
    },
    Help,
    Version,
}

/// Everything the parser can complain about.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CliError {
    #[error("{0} 需要提供取值")]
    MissingValue(String),
    #[error("未知选项 {0}")]
    UnknownOption(String),
    #[error("{0} 不是数字")]
    NotANumber(String),
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// Fails on an unknown option, a missing value, or a value that is not a number.
pub fn parse(args: &[String]) -> Result<Command, CliError> {
    let mut files: Vec<PathBuf> = Vec::new();
    let mut export: Option<(PathBuf, PathBuf, u8)> = None;
    let mut quality: u8 = 90;
    let mut install = false;
    let mut uninstall = false;
    let mut desktop_shortcut = true;
    let mut bin_dir = None;
    let mut self_test = None;
    let mut smoke = None;
    let mut new_window = false;

    let mut index = 0;
    while index < args.len() {
        let argument = args[index].clone();
        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--new-window" => new_window = true,
            "--install" => install = true,
            "--uninstall" => uninstall = true,
            "--no-desktop-shortcut" => desktop_shortcut = false,
            "--self-test" => self_test = Some(false),
            "--self-test-verbose" => self_test = Some(true),
            "--bin-dir" => bin_dir = Some(PathBuf::from(next_value(args, &mut index)?)),
            "--export" => {
                let output = PathBuf::from(next_value(args, &mut index)?);
                export = Some((PathBuf::new(), output, quality));
            }
            "--quality" => {
                let raw = next_value(args, &mut index)?;
                quality = raw
                    .parse::<u8>()
                    .map_err(|_| CliError::NotANumber("--quality".to_owned()))?
                    .clamp(1, 100);
                if let Some((input, output, _)) = export.take() {
                    export = Some((input, output, quality));
                }
            }
            "--smoke-test" => {
                let raw = next_value(args, &mut index)?;
                let frames = raw
                    .parse::<u32>()
                    .map_err(|_| CliError::NotANumber("--smoke-test".to_owned()))?;
                smoke = Some((frames, None));
            }
            "--smoke-report" => {
                let path = PathBuf::from(next_value(args, &mut index)?);
                match smoke.as_mut() {
                    Some((_, report)) => *report = Some(path),
                    None => smoke = Some((30, Some(path))),
                }
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(CliError::UnknownOption(other.to_owned()));
            }
            other => files.push(PathBuf::from(other)),
        }
        index += 1;
    }

    if let Some((input, output, quality)) = export {
        let input = if input.as_os_str().is_empty() {
            files
                .first()
                .cloned()
                .ok_or_else(|| CliError::MissingValue("--export needs an input file".to_owned()))?
        } else {
            input
        };
        return Ok(Command::Export {
            input,
            output,
            quality,
        });
    }
    if uninstall {
        return Ok(Command::Uninstall);
    }
    if install {
        return Ok(Command::Install {
            desktop_shortcut,
            bin_dir,
        });
    }
    if let Some(verbose) = self_test {
        return Ok(Command::SelfTest { verbose });
    }
    if let Some((frames, report)) = smoke {
        return Ok(Command::SmokeTest { frames, report });
    }
    Ok(Command::Run { files, new_window })
}

fn next_value(args: &[String], index: &mut usize) -> Result<String, CliError> {
    let name = args[*index].clone();
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or(CliError::MissingValue(name))
}

/// The help text, kept next to the parser so the two cannot drift apart.
#[must_use]
pub fn help_text() -> String {
    format!(
        "RustPhotoShop {version} — 开源图像编辑器（合成与修图）

用法：
    rustphotoshop [OPTIONS] [FILE...]

选项：
    -h, --help                  Show this help and exit
    -V, --version               Show the version and exit
        --new-window            Start a new window even if files are given
        --export PATH           Open the file and export it (format from the extension)
        --quality N             JPEG quality for --export, 1-100 (default 90)
        --install               Install the desktop entry, icons and a Desktop shortcut
        --no-desktop-shortcut   With --install, skip the file on the Desktop
        --bin-dir PATH          With --install, install the executable here
        --uninstall             Remove the desktop entry, icons and shortcut
        --self-test             Run the built-in checks, print a report and exit
        --self-test-verbose     The same, listing every check
        --smoke-test N          Open the window, draw N frames and exit (startup test)
        --smoke-report PATH     With --smoke-test, write a one-line result here

可打开的文件：
    .comp 工程、Photoshop 文件（.psd / .psb）以及常见图像
    (.png, .jpg, .webp, .tif, .bmp, .gif)

日志：
    RUSTPHOTOSHOP_LOG=debug        日志级别，同时写入
                                ~/.local/state/rustphotoshop/rustphotoshop.log
",
        version = crate::VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn no_arguments_starts_the_window() {
        let command = parse(&[]).expect("parse");
        assert_eq!(
            command,
            Command::Run {
                files: Vec::new(),
                new_window: false
            }
        );
    }

    #[test]
    fn files_are_collected_in_order() {
        let command = parse(&args(&["a.comp", "b.png"])).expect("parse");
        assert_eq!(
            command,
            Command::Run {
                files: vec![PathBuf::from("a.comp"), PathBuf::from("b.png")],
                new_window: false
            }
        );
    }

    #[test]
    fn help_and_version_win_over_anything_else() {
        assert_eq!(parse(&args(&["a.png", "--help"])).unwrap(), Command::Help);
        assert_eq!(parse(&args(&["--version"])).unwrap(), Command::Version);
    }

    #[test]
    fn export_takes_the_input_file_and_the_destination() {
        let command = parse(&args(&["photo.png", "--export", "out.jpg"])).expect("parse");
        assert_eq!(
            command,
            Command::Export {
                input: PathBuf::from("photo.png"),
                output: PathBuf::from("out.jpg"),
                quality: 90
            }
        );
        let tuned = parse(&args(&[
            "photo.png",
            "--export",
            "out.jpg",
            "--quality",
            "70",
        ]))
        .expect("parse");
        match tuned {
            Command::Export { quality, .. } => assert_eq!(quality, 70),
            other => panic!("wrong command: {other:?}"),
        }
    }

    #[test]
    fn install_flags_are_understood() {
        let command = parse(&args(&["--install", "--no-desktop-shortcut"])).expect("parse");
        assert_eq!(
            command,
            Command::Install {
                desktop_shortcut: false,
                bin_dir: None
            }
        );
        assert_eq!(parse(&args(&["--uninstall"])).unwrap(), Command::Uninstall);
    }

    #[test]
    fn the_test_modes_keep_their_values() {
        assert_eq!(
            parse(&args(&["--self-test"])).unwrap(),
            Command::SelfTest { verbose: false }
        );
        assert_eq!(
            parse(&args(&["--smoke-test", "12"])).unwrap(),
            Command::SmokeTest {
                frames: 12,
                report: None
            }
        );
    }

    #[test]
    fn bad_input_is_reported_clearly() {
        assert_eq!(
            parse(&args(&["--nonsense"])).unwrap_err(),
            CliError::UnknownOption("--nonsense".to_owned())
        );
        assert_eq!(
            parse(&args(&["--smoke-test"])).unwrap_err(),
            CliError::MissingValue("--smoke-test".to_owned())
        );
        assert_eq!(
            parse(&args(&["--smoke-test", "lots"])).unwrap_err(),
            CliError::NotANumber("--smoke-test".to_owned())
        );
        assert!(parse(&args(&["--export", "out.png"])).is_err());
        assert!(help_text().contains("--self-test"));
    }
}
