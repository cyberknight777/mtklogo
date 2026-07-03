use clap::{Arg, ArgAction, ArgMatches, Command};
use command::{emphasize1, err, warn};
pub use config::{Config, Format, Profile};
use std::env;
use std::io::{Error as IOError, ErrorKind, Result as IOResult};
// re-exports entry points.
use std::path::PathBuf;

mod command;
mod config;

// logo generated from http://www.patorjk.com/software/taag/#p=display&h=1&v=3&f=Doom&t=mtklogo
const LOGO: &[u8] = include_bytes!("../resources/logo.txt");

fn main() {
    match wrapped_main() {
        Ok(()) => (),
        Err(e) => {
            println!("{}: {}", warn("error"), err(e.to_string()));
            std::process::exit(1);
        }
    }
}

fn build_cli() -> Command {
    // defines common args amongst commands.
    let slots_arg = Arg::new("slots")
        .help("Extracts only these slots (others are skipped). Accepts a comma-separated list (e.g. 0,1,2) and/or ranges (e.g. 1-10).")
        .value_name("slots")
        .num_args(1)
        .long("slots")
        .conflicts_with("zip");

    let path_arg = Arg::new("path")
        .help("Path to input `logo.bin`")
        .required(true)
        .index(1)
        .value_parser(clap::builder::ValueParser::new(is_existing_file));

    Command::new("mtklogo")
        .version("0.1.2")
        .author("arlept, arnaud@lepoint.net")
        .about("Yet another Android Logo Customizer for MTK devices!\nIt packs or repacks images from an MTK `logo.bin` file.")
        .subcommand(Command::new("unpack")
            .about("Unpacks a logo image")
            .arg(Arg::new("profile")
                .help("Uses an alternative profile name")
                .value_name("profile")
                .short('p')
                .long("profile"))
            .arg(Arg::new("config")
                .help("Uses an alternative configuration file")
                .value_name("configfile")
                .num_args(1)
                .short('c')
                .long("config")
                .value_parser(clap::builder::ValueParser::new(is_existing_file)))
            .arg(Arg::new("mode")
                .help("Overrides profile's color mode")
                .value_name("mode")
                .short('m')
                .long("mode"))
            .arg(Arg::new("flip")
                .help("Flips orientation")
                .action(ArgAction::SetTrue)
                .short('f')
                .long("flip"))
            .arg(Arg::new("zip")
                .help("Do not convert to png, extract as plain .z file")
                .action(ArgAction::SetTrue)
                .short('z')
                .long("zip")
                .conflicts_with("slots"))
            .arg(Arg::new("output")
                .help("Sets images output path")
                .value_name("output")
                .num_args(1)
                .short('o')
                .long("output")
                .value_parser(clap::builder::ValueParser::new(is_existing_directory)))
            .arg(Arg::new("no-out")
                .help("Do not extract images, just checks image formats.")
                .action(ArgAction::SetTrue)
                .short('n')
                .long("no-out")
                .conflicts_with("output"))
            .arg(&path_arg)
            .arg(&slots_arg)
        )

        .subcommand(Command::new("explore")
            .about("Unpacks a logo image with the specified format\n\
this is useful is you don't know the image format, you'll probably find out.")
            .arg(Arg::new("output")
                .help("Sets images output directory")
                .value_name("output")
                .num_args(1)
                .short('o')
                .long("output")
                .value_parser(clap::builder::ValueParser::new(is_existing_directory)))
            .arg(Arg::new("width")
                .help("Image width in pixels")
                .value_name("width")
                .required(true)
                .num_args(1)
                .short('w')
                .long("width"))
            .arg(&path_arg)
            .arg(&slots_arg)
        )

        .subcommand(Command::new("guess")
            .about("Tries to guess an image dimension knowing its buffer size.\n\
Note: the program may be very slow if your input size is a large prime number!")
            .arg(Arg::new("size")
                .help("Image size in bytes")
                .value_name("size")
                .required(true)
                .num_args(1)
                .short('s')
                .long("size"))
        )

        .subcommand(Command::new("repack")
            .about("Repacks a logo image")
            .arg(Arg::new("output")
                .value_name("output")
                .help("Path to output `logo.bin`")
                .required(true)
                .num_args(1)
                .short('o')
                .long("output"))
            .arg(Arg::new("files")
                .help("Files to repack. Take care of specifying the exact set of files!")
                .value_name("files")
                .num_args(1..)
                .required(true))
            .arg(Arg::new("alpha")
                .help("Strips Alpha channel, assume image is opaque")
                .action(ArgAction::SetTrue)
                .short('a')
                .long("alpha"))
        )
}

fn wrapped_main() -> IOResult<()> {
    let mut prg = build_cli();
    let matches = prg.clone().get_matches();

    println!("{}", emphasize1(String::from_utf8_lossy(LOGO)));

    if let Some(matches) = matches.subcommand_matches("unpack") {
        let config = solve_config(matches)?;
        let profile = matches
            .get_one::<String>("profile")
            .map(|s| s.as_str())
            .unwrap_or("default")
            .to_string();
        let mode = matches.get_one::<String>("mode").map(|s| s.to_string());
        let flip = matches.get_flag("flip");
        let zip = matches.get_flag("zip");
        let check = matches.get_flag("no-out");
        let path = solve_path(matches)?;
        let output = solve_output(matches)?;
        let slots = solve_slots(matches)?;

        command::run_unpack(command::UnpackRequest {
            config,
            slots,
            profile_name: profile,
            mode,
            flip,
            zip,
            check,
            path,
            output,
        })
    } else if let Some(matches) = matches.subcommand_matches("explore") {
        let path = solve_path(matches)?;
        let output = solve_output(matches)?;
        let width = parse_or_error::<u32>(matches, "width")?;
        let slots = solve_slots(matches)?;
        command::run_explore(path, slots, output, width)
    } else if let Some(matches) = matches.subcommand_matches("repack") {
        let files = matches
            .get_many::<String>("files")
            .map(|vals| vals.cloned().collect::<Vec<_>>())
            .ok_or_else(|| IOError::other("no files to convert"))?;
        let paths = files.iter().map(PathBuf::from).collect();
        let output = matches
            .get_one::<String>("output")
            .map(PathBuf::from)
            .unwrap_or_default();
        let strip_alpha = matches.get_flag("alpha");
        command::run_repack(output, paths, strip_alpha)
    } else if let Some(matches) = matches.subcommand_matches("guess") {
        let size = parse_or_error::<usize>(matches, "size")?;
        command::run_guess(size)
    } else {
        let _ = prg.print_help();
        println!();
        Err(IOError::new(
            ErrorKind::InvalidInput,
            "unrecognized command arguments.",
        ))
    }
}

fn value_or_error(matches: &ArgMatches, label: &str) -> IOResult<String> {
    matches
        .get_one::<String>(label)
        .cloned()
        .ok_or_else(|| IOError::new(ErrorKind::InvalidInput, format!("'{}' unspecified.", label)))
}

fn parse_or_error<T>(matches: &ArgMatches, label: &str) -> IOResult<T>
where
    T: std::str::FromStr,
{
    value_or_error(matches, label).and_then(|v| {
        v.parse::<T>().map_err(|_| {
            IOError::new(
                ErrorKind::InvalidInput,
                format!("'{}' has not expected format", label),
            )
        })
    })
}

fn solve_output(matches: &ArgMatches) -> IOResult<PathBuf> {
    value_or_error(matches, "output")
        .map(PathBuf::from)
        .or_else(|_| env::current_dir())
}

fn solve_config(matches: &ArgMatches) -> IOResult<Config> {
    match matches.get_one::<String>("config") {
        Some(c) => Config::from_file(PathBuf::from(c).as_path()),
        None => Config::load(),
    }
}

fn solve_path(matches: &ArgMatches) -> IOResult<PathBuf> {
    value_or_error(matches, "path").map(PathBuf::from)
}

fn solve_slots(matches: &ArgMatches) -> IOResult<Option<Vec<usize>>> {
    match matches.get_one::<String>("slots") {
        Some(slots) => {
            let tokens: Vec<&str> = slots.split(',').collect();
            let mut sizes: Vec<usize> = Vec::with_capacity(tokens.len());
            for s in tokens.iter() {
                let trimmed = s.trim();
                if let Some((start_s, end_s)) = trimmed.split_once('-') {
                    let start = start_s.trim().parse::<usize>().map_err(|_| {
                        IOError::new(
                            ErrorKind::InvalidInput,
                            format!("'{}' is not an integer", start_s.trim()),
                        )
                    })?;
                    let end = end_s.trim().parse::<usize>().map_err(|_| {
                        IOError::new(
                            ErrorKind::InvalidInput,
                            format!("'{}' is not an integer", end_s.trim()),
                        )
                    })?;
                    if end < start {
                        return Err(IOError::new(
                            ErrorKind::InvalidInput,
                            format!("range '{}' has end before start", trimmed),
                        ));
                    }
                    sizes.extend(start..=end);
                } else {
                    let value = trimmed.parse::<usize>().map_err(|_| {
                        IOError::new(
                            ErrorKind::InvalidInput,
                            format!("'{}' is not an integer", trimmed),
                        )
                    })?;
                    sizes.push(value);
                }
            }
            Ok(Some(sizes))
        }
        None => Ok(None),
    }
}

fn is_existing_directory(val: &str) -> Result<String, String> {
    let path = PathBuf::from(val);
    std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
    Ok(val.to_owned())
}

fn is_existing_file(val: &str) -> Result<String, String> {
    if PathBuf::from(val).exists() {
        Ok(val.to_string())
    } else {
        Err(String::from("must be an existing file."))
    }
}
