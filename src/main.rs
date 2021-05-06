use anyhow::bail;
use core::f64;
use lazy_static::lazy_static;
use regex::Regex;
use std::ffi::OsStr;
use std::fs::{self, DirEntry, File};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;
use std::path::PathBuf;
use std::str;

struct Hwmon {
    name: String,
    inputs: Vec<Input>,
}

impl Hwmon {
    fn get_all() -> io::Result<Vec<Self>> {
        let hwmon_dir = Path::new("/sys/class/hwmon/");
        let mut hwmons: Vec<Self> = Vec::new();
        for dent in fs::read_dir(&hwmon_dir)?.collect::<io::Result<Vec<DirEntry>>>()? {
            let abs_path = dent.path();
            hwmons.push(Self::load(&abs_path)?);
        }
        Ok(hwmons)
    }

    fn load(dir_abs_path: &Path) -> io::Result<Self> {
        let mut name_path = dir_abs_path.to_path_buf();
        name_path.push("name");
        let name = fs::read_to_string(name_path)?;
        let mut inputs = Vec::new();
        for dent in fs::read_dir(dir_abs_path)?.collect::<io::Result<Vec<DirEntry>>>()? {
            match dent.file_name().to_str() {
                None => continue,
                Some(n) => {
                    if n.ends_with("_input") {
                        inputs.push(Input::new(&dent.path()))
                    }
                }
            }
        }
        Ok(Hwmon {
            name,
            inputs: Vec::new(),
        })
    }
}

trait Updateable {
    fn update(&self) -> f64;
    fn unit(&self) -> &str;
    fn label(&self) -> &str;
}

enum Type {
    Voltage,
    Temp,
    Fan,
    Other(Option<String>),
}

struct Input {
    f: File,
    label: String,
    typ: Type,
}

impl Input {
    fn new(input_abs_path: &Path) -> anyhow::Result<Self> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"([A-z]+)(\d+)_").unwrap();
        };
        let input_file_name = match input_abs_path.file_name().and_then(OsStr::to_str) {
            None => bail!("incorrect path {:?}", input_abs_path),
            Some(s) => s,
        };
        let parsed: regex::Captures<'_> = match RE.captures(input_file_name) {
            None => bail!("incorrect path to input {:?}", input_abs_path),
            Some(c) => c,
        };
        let typ_name = parsed.get(1).unwrap();
        let typ = match typ_name.as_str() {
            "in" => Type::Voltage,
            "fan" => Type::Fan,
            "temp" => Type::Temp,
            _ => Type::Other(Some(typ_name.as_str().to_string())),
        };
        let idx = parsed.get(2).unwrap();
        let name = &input_file_name[0..idx.end()];
        let mut label_path = input_abs_path.to_path_buf();
        label_path.pop();
        label_path.push(format!("{}_label", name));
        let label = match fs::read_to_string(label_path) {
            Err(e) => match e.kind() {
                io::ErrorKind::NotFound => name.to_string(),
                _ => return Err(e.into()),
            },
            Ok(mut s) => {
                let c = s.pop();
                assert_eq!(c, Some('\n'));
                s
            }
        };
        let f = File::open(input_abs_path)?;
        Ok(Input { f, label, typ })
    }
}

impl Updateable for Input {
    fn update(&self) -> f64 {
        let mut buf = [0u8; 4096];
        match self.f.read_at(&mut buf, 0) {
            Err(e) => {
                eprintln!("{}", e);
                0f64
            }
            Ok(n) => {
                let s = str::from_utf8(&buf[0..n - 1]).unwrap();
                let r = s.parse::<u32>().unwrap() as f64;
                match self.typ {
                    Type::Temp => r / 1000f64,
                    _ => r,
                }
            }
        }
    }

    fn label(&self) -> &str {
        &self.label
    }

    fn unit(&self) -> &str {
        use Type::*;
        match self.typ {
            Voltage => "V",
            Fan => " RPM",
            Temp => "°C",
            Other(ref m) => match m {
                None => "",
                Some(ref s) => s,
            },
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input =
        Input::new(Path::new("/sys/devices/platform/coretemp.0/hwmon/hwmon4/temp1_input").as_ref())
            .unwrap();
    println!("{}: {}{}", input.label(), input.update(), input.unit());

    let hms = Hwmon::get_all().unwrap();

    Ok(())
}
