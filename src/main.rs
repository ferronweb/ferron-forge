// Import necessary crates and modules
use std::error::Error;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;
use std::str::FromStr;

use cargo::core::compiler::{CompileKind, CompileMode, CompileTarget, UnitOutput};
use cargo::core::resolver::CliFeatures;
use cargo::core::{Verbosity, Workspace};
use cargo::ops::{CompileOptions, Packages};
use cargo::GlobalContext;
use clap::Parser;
use gix::interrupt::IS_INTERRUPTED;
use toml::Table;
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

// Struct for command-line arguments using `clap`
/// A compilation tool for easy compiling of Ferron web server
#[derive(Parser, Debug)]
#[command(name = "Ferron Forge")]
#[command(version, about, long_about = None)]
struct Args {
  /// The Ferron version or Git reference name to compile
  #[arg(short='v', long, default_value_t = String::from("main"))]
  ferron_version: String,

  /// List of modules to enable
  #[arg(short, long)]
  modules: Option<Vec<String>>,

  /// Target triple for cross-compilation
  #[arg(short, long)]
  target: Option<String>,

  /// Git repository URL containing Ferron's source code
  #[arg(short, long, default_value_t = String::from("https://github.com/ferronweb/ferron.git"))]
  repository: String,

  /// Path to the output ZIP archive
  #[arg(short, long, default_value_t = String::from("ferron-custom.zip"))]
  output: String,
}

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
  // Parse command-line arguments
  let args = Args::parse();

  println!("Creating temporary directory...");
  let temporary_directory = tempfile::tempdir()?; // Create a temporary directory

  println!("Cloning the Git repository...");
  // Clone the specified Git repository and checkout the desired ref
  let prepare_clone = gix::prepare_clone(args.repository, &temporary_directory)?;
  let (mut prepare_checkout, _) = prepare_clone
    .with_ref_name(args.ferron_version.as_str().into())?
    .fetch_then_checkout(gix::progress::Discard, &IS_INTERRUPTED)?;
  let (repo, _) = prepare_checkout.main_worktree(gix::progress::Discard, &IS_INTERRUPTED)?;

  // Determine the working directory of the repository
  let workspace_directory = match repo.workdir() {
    Some(workdir) => workdir,
    None => Err(anyhow::anyhow!("Workspace directory not found"))?,
  };

  println!("Compiling Ferron...");
  // Compile the project and retrieve the compiled binaries
  let (binaries, target_triple) = compile(
    workspace_directory.to_path_buf(),
    args.target.as_ref().map(|s| s as &str),
    args.modules.as_deref(),
  )?;

  println!("Creating ZIP archive...");
  // Set up a ZIP writer
  let zip_options = SimpleFileOptions::default();
  let zip_binary_options = SimpleFileOptions::default().unix_permissions(0o755);
  let zip_file = File::create(args.output)?;
  let mut zip = ZipWriter::new(zip_file);

  // Add each compiled binary to the ZIP
  for binary in binaries {
    let binary_path = binary.path;
    let binary_filename = match binary_path.file_name() {
      Some(filename) => filename.to_string_lossy().to_string(),
      None => continue,
    };
    let mut binary_file = File::open(binary_path)?;
    zip.start_file(binary_filename, zip_binary_options)?;
    io::copy(&mut binary_file, &mut zip)?;
  }

  // Add default configuration file to ZIP
  zip.start_file("ferron.yaml", zip_options)?;
  zip.write_all(
    r#"global:
  wwwroot: wwwroot"#
      .as_bytes(),
  )?;

  // Add `wwwroot` static assets to the ZIP
  let mut webroot_path = workspace_directory.to_path_buf();
  webroot_path.push("wwwroot");
  let walkdir_webroot = WalkDir::new(&webroot_path).into_iter();

  for entry_result in walkdir_webroot {
    let entry = entry_result?;
    let path = entry.path();
    let name = path.strip_prefix(&webroot_path).unwrap();
    let path_as_string = name.to_str().map(str::to_owned);

    if let Some(path_as_string) = path_as_string {
      if path.is_file() {
        // Add individual file to the ZIP
        zip.start_file(path_as_string, zip_options)?;
        let mut file = File::open(path)?;
        io::copy(&mut file, &mut zip)?;
      } else if !name.as_os_str().is_empty() {
        // Add directory entry to the ZIP
        zip.add_directory(path_as_string, zip_options)?;
      }
    }
  }

  // Add a comment to the ZIP metadata
  zip.set_comment(
    format!(
      "Ferron built for \"{}\" target using Ferron Forge",
      target_triple
    )
    .as_str(),
  );

  // Finalize the ZIP archive
  zip.finish()?;

  println!(
    "Built Ferron for \"{}\" target successfully!",
    target_triple
  );

  Ok(())
}

// Helper to retrieve the default Rust toolchain from rustup settings
fn get_rustup_toolchain(rustup_directory: PathBuf) -> Result<String, Box<dyn Error + Send + Sync>> {
  let mut rustup_settings_path = rustup_directory;
  rustup_settings_path.push("settings.toml");
  let rustup_settings_file = fs::read_to_string(rustup_settings_path)?;
  let rustup_settings = rustup_settings_file.parse::<Table>()?;
  let toolchain_option = rustup_settings["default_toolchain"].as_str();
  if let Some(toolchain) = toolchain_option {
    Ok(toolchain.to_string())
  } else {
    Err(anyhow::anyhow!(
      "The `rustup` configuration doesn't contain a default toolchain."
    ))?
  }
}

// Compiles the Ferron project using Cargo APIs
fn compile(
  mut workspace_directory: PathBuf,
  target: Option<&str>,
  modules: Option<&[String]>,
) -> Result<(Vec<UnitOutput>, String), Box<dyn Error + Send + Sync>> {
  let default_modules = modules.is_none();

  // Format the features for the CLI (e.g., "ferron/cgi", "ferron/cache")
  let modules_as_features = modules
    .unwrap_or(&[])
    .iter()
    .map(|feature| format!("ferron/{}", feature))
    .collect::<Vec<String>>();

  // Determine whether to compile for host or a specified target
  let compile_kind = match target {
    Some(triplet) => CompileKind::Target(CompileTarget::new(triplet)?),
    None => CompileKind::Host,
  };

  // Append `Cargo.toml` to path for creating workspace
  workspace_directory.push("Cargo.toml");

  // Set rustup environment variables for toolchain resolution
  if let Ok(rustup_home) = home::rustup_home() {
    // Safety: The std::env::set_var function is safe to call in a single-threaded program. It's called before creating global context for Cargo.
    if let Ok(toolchain) = get_rustup_toolchain(rustup_home.clone()) {
      #[allow(irrefutable_let_patterns)]
      if let Ok(toolchain) = OsString::from_str(&toolchain) {
        std::env::set_var("RUSTUP_TOOLCHAIN", toolchain);
      }
    }
    std::env::set_var("RUSTUP_HOME", rustup_home.into_os_string());
  }

  // Initialize Cargo's global context and workspace
  let global_context = GlobalContext::default()?;
  global_context.shell().set_verbosity(Verbosity::Normal);
  let workspace = Workspace::new(workspace_directory.as_path(), &global_context)?;

  // Set up compile options
  let mut compile_options = CompileOptions::new(&global_context, CompileMode::Build)?;
  compile_options.spec = Packages::All(
    workspace
      .members()
      .map(|member| member.name().to_string())
      .collect::<Vec<String>>(),
  );
  compile_options.build_config.requested_profile = "release".into();
  compile_options.build_config.requested_kinds = vec![compile_kind];

  // Add module features
  compile_options.cli_features =
    CliFeatures::from_command_line(&modules_as_features, false, default_modules)?;

  // Execute the compilation
  let compilation = cargo::ops::compile(&workspace, &compile_options)?;

  // Return the binaries and host/target triple
  Ok((compilation.binaries, compilation.host))
}
