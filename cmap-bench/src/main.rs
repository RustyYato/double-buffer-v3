use clap::Parser;
use std::{
    fmt::Debug,
    hash::Hash,
    time::{Duration, Instant},
};

#[derive(Debug, PartialEq, Eq, Hash)]
struct NotClone(i32);

#[derive(Debug, Parser)]
#[clap(rename_all = "kebab-case")]
enum Args {
    Run {
        #[clap(long)]
        min_readers: u32,
        #[clap(long)]
        max_readers: Option<u32>,
        #[clap(long)]
        min_writes: u32,
        #[clap(long)]
        max_writes: Option<u32>,
        #[clap(long)]
        timeout: f32,
    },

    RunWithConfig {
        reader_count: u32,
        write_count: u32,
        timeout: f32,
        #[clap(value_enum)]
        mode: Mode,
    },
}

#[derive(Debug, Clone, Copy, clap::ArgEnum)]
#[clap(rename_all = "kebab-case")]
enum Mode {
    CMap,
    EVMap,
}

fn parse() -> Args {
    Parser::parse()
}

fn main() {
    let args = parse();
    // dbg!(args);

    match args {
        Args::Run {
            min_readers,
            max_readers,
            min_writes,
            max_writes,
            timeout,
        } => {
            let timeout_secs = timeout;
            let timeout = timeout.to_string();
            let program = std::env::args_os().next().unwrap();
            let mut write_count = min_writes;
            while write_count <= max_writes.unwrap_or(min_writes) {
                let write_count_s = write_count.to_string();
                for reader_count in min_readers..=max_readers.unwrap_or(min_readers) {
                    let reader_count = reader_count.to_string();
                    for mode in ["c-map", "ev-map"] {
                        eprint!("run reader_count={reader_count}, write_count={write_count_s}, mode={mode}");
                        let output = std::process::Command::new(&program)
                            .args([
                                "run-with-config",
                                reader_count.as_str(),
                                write_count_s.as_str(),
                                timeout.as_str(),
                                mode,
                            ])
                            .stdout(std::process::Stdio::piped())
                            .stderr(std::process::Stdio::inherit())
                            .output()
                            .unwrap();
                        let output = String::from_utf8(output.stdout).unwrap();
                        let output =
                            output.parse::<f32>().unwrap() / timeout_secs * (write_count as f32);
                        eprintln!(
                            ", writes/s={}",
                            human_format::Formatter::new().format(output as f64)
                        );
                        println!("{reader_count}\t{write_count_s}\t{mode}\t{output}");
                    }
                    eprintln!();
                }
                write_count *= 2;
            }
        }
        Args::RunWithConfig {
            reader_count,
            write_count,
            timeout,
            mode: Mode::CMap,
        } => {
            let mut map = cmap::CMultiMap::new();

            for _ in 0..reader_count {
                let mut reader = map.reader();

                std::thread::spawn(move || {
                    reader.load();
                });
            }

            let timeout = Duration::from_secs_f32(timeout);
            let end = Instant::now() + timeout;
            let mut iter: u64 = 0;
            loop {
                iter += 1;
                for i in 0..write_count {
                    map.insert(i, i);
                }
                map.purge();

                map.flush();
                if end <= Instant::now() {
                    break;
                }
            }
            print!("{}", iter);
        }
        Args::RunWithConfig {
            reader_count,
            write_count,
            timeout,
            mode: Mode::EVMap,
        } => {
            let (mut map, reader) = evmap::new();

            map.publish();

            for _ in 0..reader_count {
                let reader = reader.clone();

                std::thread::spawn(move || {
                    reader.enter();
                });
            }

            let timeout = Duration::from_secs_f32(timeout);
            let end = Instant::now() + timeout;
            let mut iter: u64 = 0;
            loop {
                iter += 1;
                for i in 0..write_count {
                    map.insert(i, i);
                }
                map.purge();

                map.publish();
                if end <= Instant::now() {
                    break;
                }
            }

            print!("{}", iter);
        }
    }
}
