use std::{
    fmt,
    fs::{self, File},
    io,
    io::Read as _,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, bail};
use tempfile::{Builder as TempFileBuilder, NamedTempFile};

const READ_CHUNK_BYTES: usize = 64 * 1024;

pub fn write_stdout_line(arguments: fmt::Arguments<'_>) -> anyhow::Result<()> {
    let mut stdout = io::stdout().lock();
    write_line_to(&mut stdout, arguments)
}

fn write_line_to(writer: &mut dyn io::Write, arguments: fmt::Arguments<'_>) -> anyhow::Result<()> {
    writeln!(writer, "{arguments}")?;
    Ok(())
}

#[derive(Debug, Clone)]
pub enum Input {
    Path(PathBuf),
    Stdin,
}

impl FromStr for Input {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "-" {
            Ok(Self::Stdin)
        } else {
            Ok(Self::Path(PathBuf::from(s)))
        }
    }
}

impl Input {
    pub const fn is_stdin(&self) -> bool {
        matches!(self, Self::Stdin)
    }

    pub fn read_limited(&self, max_len: usize, description: &str) -> anyhow::Result<Vec<u8>> {
        let (reader, known_len): (Box<dyn io::Read>, Option<u64>) = match self {
            Self::Path(path) => {
                let known_len = path
                    .metadata()
                    .with_context(|| format!("reading {description} metadata"))?
                    .len();
                if known_len > u64::try_from(max_len)? {
                    bail!("{description} exceeds {max_len} bytes");
                }
                let reader = File::open(path).with_context(|| format!("opening {description}"))?;
                (Box::new(reader), Some(known_len))
            }
            Self::Stdin => {
                log::trace!("waiting for {description} on stdin");
                (Box::new(io::stdin()), None)
            }
        };
        read_limited_from(reader, known_len, max_len, description)
    }
}

fn read_limited_from(
    mut reader: Box<dyn io::Read>,
    known_len: Option<u64>,
    max_len: usize,
    description: &str,
) -> anyhow::Result<Vec<u8>> {
    if known_len.is_some_and(|len| len > u64::try_from(max_len).unwrap_or(u64::MAX)) {
        bail!("{description} exceeds {max_len} bytes");
    }

    let max_read_len = max_len
        .checked_add(1)
        .context("input size limit overflow")?;
    let mut data = Vec::new();
    let mut chunk = Vec::new();
    chunk
        .try_reserve_exact(READ_CHUNK_BYTES)
        .with_context(|| format!("allocating read buffer for {description}"))?;
    chunk.resize(READ_CHUNK_BYTES, 0);

    while data.len() < max_read_len {
        let remaining = max_read_len - data.len();
        let read_len = remaining.min(chunk.len());
        let count = reader
            .read(&mut chunk[..read_len])
            .with_context(|| format!("reading {description}"))?;
        if count == 0 {
            break;
        }

        data.try_reserve(count)
            .with_context(|| format!("allocating memory for {description}"))?;
        data.extend_from_slice(&chunk[..count]);
    }

    if data.len() > max_len {
        bail!("{description} exceeds {max_len} bytes");
    }
    Ok(data)
}

#[derive(Debug, Clone)]
pub enum Output {
    Path(PathBuf),
    Stdout,
}

#[derive(Debug, Clone)]
pub struct FileOutput(Output);

impl FromStr for FileOutput {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let output = value.parse::<Output>()?;
        if output.is_stdout() {
            bail!("value must be a file path, not '-'");
        }
        Ok(Self(output))
    }
}

impl FileOutput {
    pub fn write_all_new(&self, data: &[u8]) -> anyhow::Result<()> {
        self.0.write_all_new(data)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwritePolicy {
    Deny,
    Allow,
    Exclusive,
}

#[derive(Debug)]
pub enum OutputTransaction {
    Path {
        file: NamedTempFile,
        destination: PathBuf,
        directory: PathBuf,
        overwrite: OverwritePolicy,
    },
    Stdout(io::Stdout),
}

impl OutputTransaction {
    pub(crate) const fn is_stdout(&self) -> bool {
        matches!(self, Self::Stdout(_))
    }

    pub(crate) fn commit(self) -> anyhow::Result<()> {
        match self {
            Self::Path {
                mut file,
                destination,
                directory,
                overwrite,
            } => {
                io::Write::flush(&mut file)?;
                file.as_file().sync_all()?;
                let persisted = match overwrite {
                    OverwritePolicy::Allow => file.persist(destination)?,
                    OverwritePolicy::Deny => file.persist_noclobber(destination).context(
                        "output destination appeared before commit; use --force to overwrite",
                    )?,
                    OverwritePolicy::Exclusive => file
                        .persist_noclobber(destination)
                        .context("exclusive output destination appeared before commit")?,
                };
                drop(persisted);
                sync_directory(&directory)?;
                Ok(())
            }
            Self::Stdout(mut stdout) => Ok(io::Write::flush(&mut stdout)?),
        }
    }
}

impl io::Write for OutputTransaction {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Path { file, .. } => file.write(buf),
            Self::Stdout(stdout) => stdout.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Path { file, .. } => file.flush(),
            Self::Stdout(stdout) => stdout.flush(),
        }
    }
}

impl FromStr for Output {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "-" {
            Ok(Self::Stdout)
        } else {
            Ok(Self::Path(PathBuf::from(s)))
        }
    }
}

impl Output {
    pub const fn is_stdout(&self) -> bool {
        matches!(self, Self::Stdout)
    }

    pub(crate) fn begin_write(&self, force: bool) -> anyhow::Result<OutputTransaction> {
        let overwrite = if force {
            OverwritePolicy::Allow
        } else {
            OverwritePolicy::Deny
        };
        self.begin_write_with_policy(overwrite)
    }

    fn begin_write_with_policy(
        &self,
        overwrite: OverwritePolicy,
    ) -> anyhow::Result<OutputTransaction> {
        match self {
            Self::Stdout => Ok(OutputTransaction::Stdout(io::stdout())),
            Self::Path(path) => {
                match fs::symlink_metadata(path) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        bail!("output destination must not be a symbolic link")
                    }
                    Ok(metadata) if !metadata.is_file() => {
                        bail!("output destination must be a regular file")
                    }
                    Ok(_) if overwrite == OverwritePolicy::Allow => {}
                    Ok(_) => {
                        let message = match overwrite {
                            OverwritePolicy::Deny => {
                                "output destination already exists; use --force to overwrite"
                            }
                            OverwritePolicy::Exclusive => {
                                "exclusive output destination already exists"
                            }
                            OverwritePolicy::Allow => "output destination already exists",
                        };
                        return Err(io::Error::new(io::ErrorKind::AlreadyExists, message))
                            .context("opening output destination");
                    }
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error).context("inspecting output destination"),
                }
                let directory = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));

                Ok(OutputTransaction::Path {
                    file: TempFileBuilder::new()
                        .prefix(".fujicli-")
                        .suffix(".tmp")
                        .tempfile_in(directory)?,
                    destination: path.clone(),
                    directory: directory.to_owned(),
                    overwrite,
                })
            }
        }
    }

    pub fn write_all_new(&self, data: &[u8]) -> anyhow::Result<()> {
        let transaction = self.begin_write_with_policy(OverwritePolicy::Exclusive)?;
        let OutputTransaction::Path {
            mut file,
            destination,
            directory,
            ..
        } = transaction
        else {
            bail!("exclusive output requires a file path, not stdout");
        };

        io::Write::write_all(&mut file, data)?;
        io::Write::flush(&mut file)?;
        file.as_file().sync_all()?;
        drop(file.persist_noclobber(destination)?);
        sync_directory(&directory)?;
        Ok(())
    }
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, io, io::Write as _};

    use tempfile::tempdir;

    use super::{Input, Output, OutputTransaction, read_limited_from, write_line_to};

    struct BrokenWriter;

    struct PanicReader;

    impl io::Read for PanicReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            panic!("oversized regular files must be rejected before reading")
        }
    }

    impl io::Write for BrokenWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed pipe"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn text_output_propagates_broken_pipe_without_panicking() {
        let error = write_line_to(&mut BrokenWriter, format_args!("camera"))
            .expect_err("a closed stdout pipe must be reported");

        assert_eq!(
            error.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::BrokenPipe)
        );
    }

    #[test]
    fn known_oversized_input_is_rejected_before_reading_or_allocating() {
        let error = read_limited_from(Box::new(PanicReader), Some(5), 4, "test input")
            .expect_err("known oversized input must be rejected from metadata");

        assert!(error.to_string().contains("test input exceeds 4 bytes"));
    }

    #[test]
    fn output_temp_file_has_a_recoverable_product_specific_name() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("backup.dat");
        let transaction = Output::Path(destination).begin_write(false)?;
        let OutputTransaction::Path { file, .. } = transaction else {
            panic!("path output must use a temporary file");
        };
        let name = file.path().file_name().expect("temp file must have a name");

        assert!(name.to_string_lossy().starts_with(".fujicli-"));
        Ok(())
    }

    #[test]
    fn output_directory_is_rejected_before_a_transaction_is_opened() -> anyhow::Result<()> {
        let directory = tempdir()?;

        let error = Output::Path(directory.path().to_owned())
            .begin_write(false)
            .expect_err("a directory is not a valid output destination");

        assert!(error.to_string().contains("regular file"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn output_symlink_is_rejected_before_a_transaction_is_opened() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let target = directory.path().join("target.jpg");
        let link = directory.path().join("link.jpg");
        fs::write(&target, b"existing")?;
        std::os::unix::fs::symlink(&target, &link)?;

        let error = Output::Path(link)
            .begin_write(false)
            .expect_err("a symbolic-link destination must be rejected");

        assert!(error.to_string().contains("symbolic link"));
        assert_eq!(fs::read(target)?, b"existing");
        Ok(())
    }

    #[test]
    fn bounded_input_accepts_limit_and_rejects_limit_plus_one() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let accepted = directory.path().join("accepted.dat");
        let rejected = directory.path().join("rejected.dat");
        fs::write(&accepted, b"1234")?;
        fs::write(&rejected, b"12345")?;

        assert_eq!(
            Input::Path(accepted).read_limited(4, "test input")?,
            b"1234"
        );
        let error = Input::Path(rejected)
            .read_limited(4, "test input")
            .expect_err("limit + 1 must be rejected");
        assert!(
            error.to_string().contains("test input exceeds 4 bytes"),
            "unexpected error: {error:#}"
        );
        Ok(())
    }

    #[test]
    fn dropping_uncommitted_output_preserves_existing_file() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("backup.dat");
        fs::write(&destination, b"existing backup")?;

        let mut output = Output::Path(destination.clone()).begin_write(true)?;
        output.write_all(b"replacement backup")?;
        drop(output);

        assert_eq!(fs::read(destination)?, b"existing backup");
        Ok(())
    }

    #[test]
    fn output_preserves_existing_file_without_explicit_overwrite() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("backup.dat");
        fs::write(&destination, b"existing backup")?;

        let error = Output::Path(destination.clone())
            .begin_write(false)
            .expect_err("ordinary output must not overwrite an existing file");

        assert!(error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::AlreadyExists)
        }));
        assert_eq!(fs::read(destination)?, b"existing backup");
        Ok(())
    }

    #[test]
    fn explicit_overwrite_atomically_replaces_existing_file() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("backup.dat");
        fs::write(&destination, b"existing backup")?;

        let mut output = Output::Path(destination.clone()).begin_write(true)?;
        output.write_all(b"replacement backup")?;
        output.commit()?;

        assert_eq!(fs::read(destination)?, b"replacement backup");
        Ok(())
    }

    #[test]
    fn concurrent_creator_wins_when_overwrite_is_not_allowed() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("backup.dat");
        let mut output = Output::Path(destination.clone()).begin_write(false)?;
        output.write_all(b"generated backup")?;
        fs::write(&destination, b"concurrent backup")?;

        let error = output
            .commit()
            .expect_err("commit must not overwrite a concurrently-created file");

        assert!(error.to_string().contains("--force"), "{error:#}");
        assert!(error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::AlreadyExists)
        }));
        assert_eq!(fs::read(destination)?, b"concurrent backup");
        Ok(())
    }

    #[test]
    fn exclusive_output_atomically_creates_new_file() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("recovery.fbk");

        Output::Path(destination.clone()).write_all_new(b"recovery backup")?;

        assert_eq!(fs::read(destination)?, b"recovery backup");
        Ok(())
    }

    #[test]
    fn exclusive_output_preserves_existing_recovery_file() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("recovery.fbk");
        fs::write(&destination, b"existing recovery")?;

        let error = Output::Path(destination.clone())
            .write_all_new(b"replacement")
            .expect_err("recovery output must never clobber an existing file");

        let diagnostic = format!("{error:#}");
        assert!(
            !diagnostic.contains("--force"),
            "exclusive recovery output must not suggest an unavailable override: {diagnostic}"
        );
        assert!(error.chain().any(|cause| {
            cause
                .downcast_ref::<io::Error>()
                .is_some_and(|error| error.kind() == io::ErrorKind::AlreadyExists)
        }));
        assert_eq!(fs::read(destination)?, b"existing recovery");
        Ok(())
    }
}
