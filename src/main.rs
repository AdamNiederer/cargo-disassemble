// cargo-disassemble: Easily disassemble Rust programs
// Copyright (C) 2018 Adam Niederer

// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.

// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

// TODO: https://github.com/rust-lang/cargo/issues/5295

#![allow(unused_imports)]

extern crate rustc_demangle;
extern crate regex;
extern crate structopt;
extern crate glob;
extern crate toml;
extern crate cargo;

#[macro_use]
extern crate structopt_derive;

use rustc_demangle::try_demangle;
use structopt::StructOpt;
use glob::glob;
use std::fs::{File, OpenOptions, remove_file};
use std::io::prelude::*;
use std::io::{BufReader, BufRead, SeekFrom};
use std::env::{current_dir, set_current_dir};
use std::path::Path;
use std::process::{Command, Stdio};
use std::string::ParseError;
use toml::Value;
use regex::Regex;
use cargo::ops::{CompileOptions, CleanOptions, Packages, CompileFilter, MessageFormat, CompileMode};
use cargo::core::Workspace;
use cargo::core::shell::Verbosity;
use cargo::util::config::Config;

fn parse_opt_vec(src: &str) -> Result<Vec<String>, ParseError> {
    Ok(src.split_whitespace().map(Into::into).collect::<Vec<String>>())
}

#[derive(StructOpt, Debug)]
#[structopt(name = "cargo-disassemble", about = "Easy disassembly of Rust code.")]
struct Options {
    #[structopt(help = "The name of the function to be decompiled")]
    function: Option<String>,
    #[structopt(long = "everything", help = "Include functions not defined by the current crate")]
    everything: bool,
    #[structopt(long = "release", help = "Compile in release mode")]
    release: bool,
    #[structopt(long = "intel", help = "Emit intel-flavored x86 ASM")]
    intel: bool,
    #[structopt(long = "optimize", help = "Optimize the binary as much as possible")]
    optimize: bool,
    #[structopt(long = "features", help = "Features to enable, if any", parse(try_from_str = "parse_opt_vec"))]
    features: Option<Vec<String>>,
    #[structopt(long = "all-features", help = "Enable all features")]
    all_features: bool,
    #[structopt(long = "no-default-features", help = "Enable no_default features")]
    no_default_features: bool
}

fn main() {
    let options = Options::from_args();

    while !Path::new("Cargo.toml").exists() {
        if current_dir().unwrap() == Path::new("/") {
            eprintln!("cargo-dissassemble: could not find Cargo.toml");
            return;
        }
        set_current_dir(Path::new("..")).unwrap();
    }

    let cargo_toml = toml::from_str::<Value>(&{
        let mut buf = String::new();
        let mut cargo_file = File::open("Cargo.toml").expect("Failed to open Cargo.toml.");
        cargo_file.read_to_string(&mut buf).expect("Failed to read Cargo.toml");
        buf
    }).expect("Failed to parse Cargo.toml");

    let package_name = if let Value::Table(ref toplevel) = cargo_toml {
        if let Some(&Value::Table(ref package)) = toplevel.get("package") {
            if let Some(&Value::String(ref name)) = package.get("name") {
                name
            } else {
                panic!("Could not parse package name");
            }
        } else {
            panic!("Could not parse package name");
        }
    } else {
        panic!("Could not parse package name");
    }.replace("-", "_");

    let rustc_opts = {
        let mut opts = vec!("--emit", "asm", "-C", "debuginfo=2");

        if options.optimize {
            opts.extend(&["-C", "target-cpu=native", "-C", "opt-level=3"]);
        }

        if options.intel {
            opts.push("-Cllvm-args=--x86-asm-syntax=intel");
        }

        opts
    }.iter().map(|s| (*s).into()).collect::<Vec<String>>();

    // UPSTREAM BUG TODO: https://github.com/rust-lang/cargo/issues/5295

    // let conf = {
    //     let mut conf = Config::default().unwrap();
    //     conf.shell().set_verbosity(Verbosity::Quiet);
    //     conf
    // };
    //
    // let features = &options.features.unwrap_or(Vec::new());
    // let ws = Workspace::new(&current_dir().unwrap().join("Cargo.toml"), &conf).unwrap();
    //
    // cargo::ops::clean(&ws, &CleanOptions {
    //     spec: &[package_name.clone().replace("_", "-")],
    //     target: None,
    //     config: &conf,
    //     release: false
    // }).unwrap();

    let mut clean = Command::new("cargo");
    clean.arg("clean");
    if options.release {
        clean.arg("--release");
    }
    clean.args(&["-p", &package_name.replace("_", "-")]);
    clean.stderr(Stdio::null()).stdout(Stdio::null())
        .status().expect("Failed to execute cargo clean");

    // UPSTREAM BUG TODO: https://github.com/rust-lang/cargo/issues/5295
    // let compilation = cargo::ops::compile(&ws, &CompileOptions {
    //     config: &conf,
    //     jobs: None,
    //     target: None,
    //     features: features,
    //     all_features: options.all_features,
    //     no_default_features: options.no_default_features,
    //     spec: Packages::All,
    //     release: options.release,
    //     filter: CompileFilter::Default { required_features_filterable: true },
    //     mode: CompileMode::Build,
    //     message_format: MessageFormat::Human,
    //     target_rustdoc_args: None,
    //     target_rustc_args: Some(&rustc_opts)
    // }).unwrap();

    // Compile
    let mut rustc = Command::new("cargo");
    rustc.arg("rustc");
    if options.release {
        rustc.arg("--release");
    }
    rustc.arg("--");
    rustc.args(&rustc_opts);
    rustc.stderr(Stdio::null()).stdout(Stdio::null())
        .status().expect("Failed to execute cargo rustc");

    {
        let mut master_file = File::create("./tlobdog-master.s").expect("Failed to open master file");

        // UPSTREAM BUG TODO: https://github.com/rust-lang/cargo/issues/5295
        // for path in glob(compilation.deps_output.join(package_name.clone() + "-*.d").to_str().unwrap()).unwrap().map(Result::unwrap)
        //     .chain(glob(compilation.deps_output.join(package_name.clone() + "-*.s").to_str().unwrap()).unwrap().map(Result::unwrap)) {

        let search_path = if options.release {
            glob("target/release/deps/*.d").unwrap()
                .chain(glob("target/release/deps/*.s").unwrap())
                .map(Result::unwrap)
        } else {
            glob("target/debug/deps/*.d").unwrap()
                .chain(glob("target/debug/deps/*.s").unwrap())
                .map(Result::unwrap)
        };

        for path in search_path {
            let mut file = File::open(&path).expect(&format!("Failed to open {}", &path.to_str().unwrap()));
            let mut file_contents = String::new();
            file.read_to_string(&mut file_contents).expect(&format!("Failed to read {}", &path.to_str().unwrap()));
            master_file.write(&file_contents.as_bytes()).expect("Failed to write to master file");

            remove_file(&path).expect(&format!("Failed to remove {}", &path.to_str().unwrap()));
        }
    }

    let master_file = File::open("./tlobdog-master.s").expect("Failed to open master file");
    let mut prasm = false;

    let print_re = match options.function {
        Some(desired) => Regex::new(&desired).expect("That function name isn't a valid regex."),
        None => Regex::new(".*").unwrap(),
    };

    for line in BufReader::new(master_file).lines().map(Result::unwrap) {

        if let Ok(func) = try_demangle(&line[..line.len().saturating_sub(1)]) {
            let func = format!("{}", func);
            if (options.everything || func.starts_with(&package_name)) && print_re.is_match(&func) {
                prasm = true;
                println!("{}", func.split_at(func.rfind(":").unwrap() - 1).0);
            }

            continue;
        }

        if prasm && (is_branch_label(&line) || is_instruction(&line)) {
            if line.trim_left().starts_with("call") {
                if let Ok(func) = try_demangle(line.split("\t").last().unwrap()) {
                    let func = format!("{}", func);
                    println!("\tcall\t{}", func.split_at(func.rfind(":").unwrap() - 1).0);
                }
            } else {
                println!("{}", line);
            }
            if line.trim_left().starts_with("ret") {
                prasm = false;
            }
        }
    }

    remove_file("./tlobdog-master.s").expect(&format!("Failed to remove master file"));
}

fn is_branch_label(line: &str) -> bool {
    line.starts_with(".LBB")
}

fn is_instruction(line: &str) -> bool {
    (line.starts_with(" ") || line.starts_with("\t"))
        && !line.trim_left().starts_with(".")
}
