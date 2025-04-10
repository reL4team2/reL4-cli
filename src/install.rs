use anyhow::Ok;
use clap::Parser;
use std::{process::Command, vec};

#[derive(Debug, Parser)]
pub(crate) struct InstallOptions {
    #[clap(subcommand)]
    command: InstallCommand,
}

#[derive(Debug, Parser)]
enum InstallCommand {
    /// Install reL4 kernel, libseL4, kernel loader, which needs by the userspace development
    #[command(about = "Install reL4 kernel, libseL4, kernel loader")]
    Kernel(KernelOptions),
}

pub(crate) fn install(opts: InstallOptions) -> anyhow::Result<()> {
    match opts.command {
        InstallCommand::Kernel(kernel_opts) => {
            install_kernel(&kernel_opts, &kernel_opts.sel4_prefix)?;
            install_kernel_loader(&kernel_opts, &kernel_opts.sel4_prefix)?;
        }
    }
    Ok(())
}

#[derive(Debug, Parser)]
struct KernelOptions {
    /// The target platform to install
    #[clap(default_value = "qemu-arm-virt", short, long)]
    pub platform: String,
    /// Enable kernel mcs mode
    #[clap(short, long)]
    pub mcs: bool,
    /// Disable fastpath
    #[clap(long)]
    pub nofastpath: bool,
    /// Rel4 has two modes:
    /// - Binary mode (pure Rust)
    /// - Lib mode (integrates with seL4 kernel)
    ///
    /// Currently, the default is lib mode. Binary mode is still in development.
    /// If you want to use binary mode, please set this option.
    #[clap(long, short = 'B')]
    pub bin: bool,
    /// seL4 prefix path
    #[clap(short = 'P', long, default_value = "/workspace/.seL4")]
    pub sel4_prefix: String,
    /// Local reL4 kernel path
    #[clap(short = 'L', long)]
    pub local: Option<String>,
    /// rel4 kernel branch
    #[clap(long, default_value = "master")]
    pub branch: String,
    /// force install
    #[clap(long)]
    pub force: bool,
    /// seL4 baseline version
    #[clap(long)]
    pub sel4_baseline: Option<String>,
}

/// Install kernel, seL4 or reL4
fn install_kernel(opts: &KernelOptions, prefix: &str) -> anyhow::Result<()> {
    if let Some(commit) = &opts.sel4_baseline {
        install_sel4_kernel(opts, prefix, &commit)
    } else {
        install_rel4_kernel(opts, prefix)
    }
}

/// Install seL4 kernel
fn install_sel4_kernel(opts: &KernelOptions, prefix: &str, commit: &str) -> anyhow::Result<()> {
    let path = "/tmp/seL4_kernel";
    if std::fs::remove_dir_all(path).is_err() {
        // Do nothing if the directory does not exist
    }

    let mut exec = Command::new("git");
    let command = exec.args(&[
        "clone",
        "https://github.com/seL4/seL4.git",
        path,
    ]);

    let mut attempts = 0;
    while !command.status()?.success() && attempts < 3 {
        attempts += 1;
        eprintln!("seL4 git clone failed. Retrying... (attempt {}/{})", attempts, 3);
    }

    let checkout_command = Command::new("git")
        .args(&["checkout", commit])
        .current_dir(path)
        .status()?;
    if !checkout_command.success() {
        return Err(anyhow::anyhow!("Failed to checkout specific commit"));
    }

    let build_sel4_dir = std::path::PathBuf::from(path);

    let build_sel4_dir = std::fs::canonicalize(build_sel4_dir)?;
    let sel4_build_path = build_sel4_dir.join("build");

    let install_prefix_flag = format!("-DCMAKE_INSTALL_PREFIX={}", prefix);
    let args: Vec<&str> = match opts.platform.as_str() {
        "spike" => {
            vec![
                "-DCROSS_COMPILER_PREFIX=riscv64-unknown-linux-gnu-",
                &install_prefix_flag,
                "-DKernelArch=riscv",
                "-DKernelPlatform=spike",
                "-DKernelSel4Arch=riscv64",
                "-DKernelVerificationBuild=OFF",
                "-G", "Ninja",
                "-S", ".",
                "-B", sel4_build_path.to_str().unwrap(),
            ]
        },
        "qemu-arm-virt" => {
            vec![
                "-DCROSS_COMPILER_PREFIX=aarch64-linux-gnu-",
                "-DKernelAllowSMCCalls=ON",
                &install_prefix_flag,
                "-DKernelArmExportPCNTUser=ON",
                "-DKernelArmExportPTMRUser=ON",
                "-DARM_CPU=cortex-a57",
                "-DKernelArch=arm",
                "-DKernelArmHypervisorSupport=OFF",
                "-DKernelPlatform=qemu-arm-virt",
                "-DKernelSel4Arch=aarch64",
                "-DKernelVerificationBuild=OFF",
                "-G", "Ninja",
                "-S", ".",
                "-B", sel4_build_path.to_str().unwrap(),
            ]
        },
        _ => return Err(anyhow::anyhow!("Unsupported platform")),
        
    };

    let status = Command::new("cmake")
        .args(args)
        .current_dir(build_sel4_dir.clone())
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to configure project with CMake"));
    }

    let status = Command::new("ninja")
        .args(&["-C", "build", "all"])
        .current_dir(build_sel4_dir.clone())
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to build project with Ninja"));
    }

    let status = Command::new("ninja")
        .args(&["-C", "build", "install"])
        .current_dir(build_sel4_dir)
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to install project with Ninja"));
    }

    Ok(())
}

/// Install rel4 kernel stuff
/// If Binary mode is enabled, reL4 kernel build kernel.elf and install it
/// If Lib mode is enabled, reL4 kernel build librustlib.a for seL4 kernel
fn install_rel4_kernel(opts: &KernelOptions, prefix: &str) -> anyhow::Result<()> {
    let rel4_kernel_dir = 
    if let Some(local_path) = &opts.local {
        local_path.as_str()
    } else {
        let path = "/tmp/rel4_kernel";
        if opts.force || !std::path::Path::new(path).exists() {
            if std::fs::remove_dir_all(path).is_err() {
                // Do nothing if the directory does not exist
            }

            let mut exec = Command::new("git");
            let command = exec
                .args(&["clone", "https://github.com/reL4team2/rel4-integral.git", path, 
                        "--config", "advice.detachedHead=false", "--depth", "1", "--branch", &opts.branch]);
            let mut attempts = 0;
            while !command.status()?.success() && attempts < 3 {
                attempts += 1;
                eprintln!("rel4-integral git clone failed. Retrying... (attempt {}/{})", attempts, 3);
            }

            // fix home version bug
            let status = Command::new("cargo").args(&["update", "home@0.5.11", "--precise", "0.5.5"]).current_dir(path).status()?;
            if !status.success() {
                return Err(anyhow::anyhow!("Failed to update home version"));
            }
        }
    
        path
    };

    let mut command = Command::new("rustup");
    let mut args = vec![
        "run",
        "nightly-2024-02-01",
        "cargo",
        "xtask",
        "build",
        "--rust-only",
    ];

    match opts.platform.as_str() {
        "spike" => {
            args.push("--platform");
            args.push("spike");
        }
        "qemu-arm-virt" => {
            args.push("--platform");
            args.push("qemu-arm-virt");
            args.push("-s");
            args.push("on");
            args.push("--arm-pcnt");
            args.push("--arm-ptmr");
        }
        _ => {
            return Err(anyhow::anyhow!("Unsupported platform: {}", opts.platform));
        }
    }

    if opts.mcs {
        args.push("--mcs");
        args.push("on");
    }

    if opts.nofastpath {
        args.push("--nofastpath");
    }

    if opts.bin {
        args.push("--bin");
    }
    
    if !command.args(&args).current_dir(rel4_kernel_dir).status()?.success() {
        return Err(anyhow::anyhow!("Failed to build reL4 kernel"));
    }

    if opts.bin {
        let target: String = match opts.platform.as_str() {
            "spike" => {"riscv64imac-unknown-none-elf".to_string()},
            "qemu-arm-virt" => {"aarch64-unknown-none-softfloat".to_string()},
            _ => return Err(anyhow::anyhow!("Unsupported platform")),
        };
        let kernel_path = std::path::PathBuf::from(&rel4_kernel_dir).join(format!("target/{}/release/rel4_kernel", target));
        let install_path = std::path::PathBuf::from(&prefix).join("bin/kernel.elf");
        std::fs::create_dir_all(install_path.parent().ok_or_else(|| anyhow::anyhow!("Invalid install path"))?)?;
        std::fs::copy(&kernel_path, &install_path)?;
    }

    let build_sel4_dir = 
    if let Some(local_path) = &opts.local {
        std::path::PathBuf::from(local_path).join("../kernel")
    } else {
        let path = "/tmp/seL4_kernel";
        if opts.force || !std::path::Path::new(path).exists() {
            if std::fs::remove_dir_all(path).is_err() {
                // Do nothing if the directory does not exist
            }

            let mut exec = Command::new("git");
            let command = exec.args(&["clone", "https://github.com/reL4team2/seL4_c_impl.git", path, "--config", "advice.detachedHead=false"]);
            let mut attempts = 0;
            while !command.status()?.success() && attempts < 3 {
                attempts += 1;
                eprintln!("seL4_c_impl git clone failed. Retrying... (attempt {}/{})", attempts, 3);
            }
        }
        std::path::PathBuf::from(path)
    };

    let build_sel4_dir = std::fs::canonicalize(build_sel4_dir)?;    
    let sel4_build_path = build_sel4_dir.join("build");

    let rel4_kernel_flag = format!("-DREL4_KERNEL={}", if opts.bin { "TRUE" } else { "FALSE" });
    let install_prefix_flag = format!("-DCMAKE_INSTALL_PREFIX={}", prefix);
    let args: Vec<&str> = match opts.platform.as_str() {
        "spike" => {
            vec![
                "-DCROSS_COMPILER_PREFIX=riscv64-unknown-linux-gnu-",
                &install_prefix_flag, &rel4_kernel_flag,
                "-C", "./kernel-settings-riscv64.cmake",
                "-G", "Ninja",
                "-S", ".",
                "-B", sel4_build_path.to_str().unwrap(),
            ]
        },
        "qemu-arm-virt" => {
            vec![
                "-DCROSS_COMPILER_PREFIX=aarch64-linux-gnu-",
                "-DKernelAllowSMCCalls=ON",
                &install_prefix_flag, &rel4_kernel_flag,
                "-DKernelArmExportPCNTUser=ON",
                "-DKernelArmExportPTMRUser=ON",
                "-C", "./kernel-settings-aarch64.cmake",
                "-G", "Ninja",
                "-S", ".",
                "-B", sel4_build_path.to_str().unwrap(),
            ]
        },
        _ => return Err(anyhow::anyhow!("Unsupported platform")),
        
    };

    let status = Command::new("cmake")
        .args(args)
        .current_dir(build_sel4_dir.clone())
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to configure project with CMake"));
    }

    let status = Command::new("ninja")
        .args(&["-C", "build", "all"])
        .current_dir(build_sel4_dir.clone())
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to build project with Ninja"));
    }

    let status = Command::new("ninja")
        .args(&["-C", "build", "install"])
        .current_dir(build_sel4_dir)
        .status()?;
    if !status.success() {
        return Err(anyhow::anyhow!("Failed to install project with Ninja"));
    }

    Ok(())
}

fn install_kernel_loader(opts: &KernelOptions, prefix: &str) -> anyhow::Result<()> {
    let mut cmd = Command::new("rustup");
    let url: String = "https://github.com/reL4team2/rust-sel4.git".into();
    let rev: String = "642b58d807c5e5fc22f0c15d1467d6bec328faa9".into();

    let mut args: Vec<&str> = vec![
        "run",
        "nightly-2024-08-01",
        "cargo",
        "install",
        "--git", url.as_str(),
        "--rev", rev.as_str(),
        "--root".into(), prefix,
        "sel4-kernel-loader-add-payload".into(),
    ];

    if opts.force {
        args.push("--force");
    }

    cmd.env_remove("RUSTUP_TOOLCHAIN").env_remove("CARGO").args(&args).status().expect("failed install sel4-kernel-loader-add-payload");
    
    let target: String = match opts.platform.as_str() {
        "spike" => {"riscv64imac-unknown-none-elf".to_string()},
        "qemu-arm-virt" => {"aarch64-unknown-none".to_string()},
        _ => return Err(anyhow::anyhow!("Unsupported platform")),
    };
    let mut cmd = Command::new("rustup");
    let mut args: Vec<&str>  = vec![
        "run",
        "nightly-2024-08-01",
        "cargo",
        "install",
        "-Z", "build-std=core,compiler_builtins",
        "-Z", "build-std-features=compiler-builtins-mem",
        "--target", target.as_str(),
        "--git", url.as_str(),
        "--rev", rev.as_str(),
        "--root".into(), prefix,
        "sel4-kernel-loader".into(),
    ];

    if opts.force {
        args.push("--force");
    }

    cmd.env_remove("RUSTUP_TOOLCHAIN")
        .env_remove("CARGO")
        .env("SEL4_PREFIX", prefix)
        .env("CC_aarch64_unknown_none", "aarch64-linux-gnu-gcc")
        .args(&args)
        .status().expect("failed install sel4-kernel-loader");

    Ok(())
}