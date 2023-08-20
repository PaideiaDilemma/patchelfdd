use patchelfdd::{opts::Opts, patchelfdd::Error, run};

use std::{fs, path::PathBuf, process::Command};

const TEST_INTERPPATH: &str = "/lib-sus.so";
const NATIVE_LIBC64: &str = "/lib64/libc.so.6";
const NATIVE_LIBC32: &str = "/usr/lib32/libc.so.6";

#[test]
fn patch_minimal_amd64() -> Result<(), Error> {
    test_prebuild_patch("./tests/prebuild/minimal-amd64", LIBC::ELF64)?;
    Ok(())
}

#[test]
fn patch_minimal_i386() -> Result<(), Error> {
    test_prebuild_patch("./tests/prebuild/minimal-i386", LIBC::ELF32)?;
    Ok(())
}

#[test]
fn patch_itm_gprof_amd64() -> Result<(), String> {
    match test_prebuild_patch("./tests/prebuild/itm-gprof-amd64", LIBC::ELF64) {
        Ok(_) => Err("Should fail".to_string()),
        Err(_) => Ok(()),
    }
}

enum LIBC {
    ELF32,
    ELF64,
}

fn setup(scratch_dir: &PathBuf, libc: LIBC) {
    fs::create_dir_all(&scratch_dir).expect("Failed to create directory");
    let local_libc = scratch_dir.join("libc.so.6");
    match libc {
        LIBC::ELF32 => {
            fs::copy(NATIVE_LIBC32, local_libc).expect("Failed to copy native libc");
        }
        LIBC::ELF64 => {
            fs::copy(NATIVE_LIBC64, local_libc).expect("Failed to copy native libc");
        }
    }
}

fn verify_patches_with_ldd(executable_path: &PathBuf, scratch_dir: &str) {
    let output = Command::new("ldd")
        .arg(executable_path)
        .output()
        .expect("Faild to run ldd");

    assert!(output.status.success());
    let output_string = String::from_utf8_lossy(&output.stdout).into_owned();
    let output_lines = output_string.lines();

    let mut correct_runpath: bool = false;
    let mut correct_interppath: bool = false;
    for (_, line) in output_lines.enumerate() {
        dbg!(line);
        if line.contains(&format!("{}/libc.so.6", scratch_dir)) {
            correct_runpath = true;
        } else if line.contains(TEST_INTERPPATH) {
            correct_interppath = true;
        }
    }

    assert!(correct_runpath);
    assert!(correct_interppath);
}

fn test_prebuild_patch(prebuild_path: &str, libc: LIBC) -> Result<(), Error> {
    let path = PathBuf::from(prebuild_path);
    let scratch_dir = PathBuf::from(match libc {
        LIBC::ELF32 => "/tmp/elf32dd",
        LIBC::ELF64 => "/tmp/elf64dd",
    });

    setup(&scratch_dir, libc);

    let file_name = path.file_name().expect("Failed to get executable name");
    let scratch_executable = scratch_dir.join(file_name);
    fs::copy(path, &scratch_executable).expect("Failed to copy executable to tmpdir");
    let opts = Opts {
        bin: scratch_executable.clone(),
        set_runpath: Some(scratch_dir.to_string_lossy().to_string()),
        set_interpreter: Some(TEST_INTERPPATH.to_string()),
    };

    run(opts)?;

    verify_patches_with_ldd(&scratch_executable, &scratch_dir.to_string_lossy());

    Ok(())
}
