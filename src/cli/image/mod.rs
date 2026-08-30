use fujicli::{
    features::render::{RenderSaveError, finish_render_cleanup, validate_xt5_raf},
    generated::{cli::RenderArgs, renders::RenderBase},
    policy::{EmulationAcknowledgement, SerialFingerprint},
};

use super::common::file::{Input, Output, OutputTransaction};
use crate::cli::{
    GlobalOptions,
    common::{interrupt, usb},
};
use clap::Subcommand;

const MAX_IMAGE_INPUT_BYTES: usize = 512 * 1024 * 1024;

trait RenderOutput: std::io::Write {
    fn commit(self) -> anyhow::Result<()>;
}

impl RenderOutput for OutputTransaction {
    fn commit(self) -> anyhow::Result<()> {
        self.commit()
    }
}

fn save_rendered_object<Output: RenderOutput>(
    mut output: Output,
    handle: u32,
    data: &[u8],
    profile_restore_error: Option<anyhow::Error>,
    delete_after_save: bool,
    cleanup: impl FnOnce(u32) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut profile_restore_error = profile_restore_error;
    if let Err(save) = std::io::Write::write_all(&mut output, data) {
        return Err(RenderSaveError::new(handle, save.into(), profile_restore_error.take()).into());
    }
    if let Err(save) = output.commit() {
        return Err(RenderSaveError::new(handle, save, profile_restore_error.take()).into());
    }

    if !delete_after_save {
        let mut stderr = std::io::stderr().lock();
        std::io::Write::write_fmt(
            &mut stderr,
            format_args!(
                "rendered JPEG saved; camera object {handle} was retained; recover it with `fujicli image recover {handle} OUTPUT --target-serial-sha256 SHA256`\n"
            ),
        )?;
        return finish_render_cleanup(handle, profile_restore_error, Ok(()));
    }

    finish_render_cleanup(handle, profile_restore_error, cleanup(handle))
}

#[derive(Subcommand, Debug)]
pub enum ImageCmd {
    /// Render image
    #[command(alias = "r")]
    Render {
        /// SHA-256 fingerprint of the exact physical camera serial number
        #[arg(long, required_unless_present = "emulate")]
        target_serial_sha256: Option<SerialFingerprint>,

        /// Path to exported simulation file
        #[arg(long)]
        simulation_file: Option<Input>,

        /// Render a lower-quality (faster) preview
        #[arg(long)]
        draft: bool,

        #[command(flatten)]
        render: RenderArgs,

        /// RAF input file (use '-' to read from stdin)
        input: Input,

        /// Output file (use '-' to write to stdout)
        output: Output,
    },

    /// Recover a retained rendered JPEG by its camera object handle
    Recover {
        /// SHA-256 fingerprint of the exact physical camera serial number
        #[arg(long, required_unless_present = "emulate")]
        target_serial_sha256: Option<SerialFingerprint>,

        /// Camera object handle reported by a failed render
        handle: u32,

        /// Output file (use '-' to write to stdout)
        output: Output,

        /// Delete the camera object after a verified local file save
        #[arg(long)]
        delete_after_save: bool,
    },
}

struct RenderRequest {
    target_serial_sha256: Option<SerialFingerprint>,
    simulation_file: Option<Input>,
    draft: bool,
    render: RenderArgs,
    input: Input,
    output: Output,
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the handler consumes the flattened CLI render command"
)]
fn handle_render(options: GlobalOptions, request: RenderRequest) -> anyhow::Result<()> {
    let RenderRequest {
        target_serial_sha256,
        simulation_file,
        draft,
        render,
        input,
        output,
    } = request;
    let GlobalOptions {
        device, emulate, ..
    } = options;

    let image = input.read_limited(MAX_IMAGE_INPUT_BYTES, "RAF image")?;
    validate_xt5_raf(&image)?;
    let simulation_json = simulation_file
        .map(|file| {
            file.read_limited(
                super::simulation::MAX_SIMULATION_INPUT_BYTES,
                "simulation JSON",
            )
        })
        .transpose()?;
    let output_transaction = output.begin_write()?;
    let delete_after_save = !output_transaction.is_stdout();

    let mut camera = usb::get_camera(device, emulate, EmulationAcknowledgement::NotProvided)?;
    let simulation_from_file = if let Some(buffer) = simulation_json {
        Some(camera.deserialize_simulation(&buffer)?.to_base())
    } else {
        None
    };
    let target_serial_sha256 = target_serial_sha256
        .ok_or_else(|| anyhow::anyhow!("RAW conversion requires --target-serial-sha256"))?;
    let mut session = camera.preflight_raw_conversion(&target_serial_sha256)?;

    let mut base = RenderBase::default();
    if let Some(sim) = simulation_from_file {
        base.try_update_from(&sim);
    }
    base.merge(render.into());

    let outcome = interrupt::critical_camera_write("RAW render upload", || {
        session.render(&image, base, draft)
    })?;
    let (rendered, profile_restore_error) = outcome.into_parts();
    save_rendered_object(
        output_transaction,
        rendered.handle(),
        rendered.data(),
        profile_restore_error,
        delete_after_save,
        |handle| session.cleanup_rendered_object(handle).map(|_| ()),
    )
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "the handler consumes the parsed recovery command"
)]
fn handle_recover(
    options: GlobalOptions,
    target_serial_sha256: Option<SerialFingerprint>,
    handle: u32,
    output: Output,
    delete_after_save: bool,
) -> anyhow::Result<()> {
    let GlobalOptions {
        device, emulate, ..
    } = options;
    anyhow::ensure!(
        !(delete_after_save && output.is_stdout()),
        "--delete-after-save requires a file output, not stdout"
    );
    let output_transaction = output.begin_write()?;
    let target_serial_sha256 = target_serial_sha256
        .ok_or_else(|| anyhow::anyhow!("RAW recovery requires --target-serial-sha256"))?;

    let mut camera = usb::get_camera(device, emulate, EmulationAcknowledgement::NotProvided)?;
    let rendered = camera
        .preflight_raw_recovery_fetch(&target_serial_sha256)?
        .recover_rendered_object(handle)?;

    save_rendered_object(
        output_transaction,
        rendered.handle(),
        rendered.data(),
        None,
        delete_after_save,
        |handle| {
            camera
                .preflight_raw_recovery_cleanup(&target_serial_sha256)?
                .cleanup_rendered_object(handle)
                .map(|_| ())
        },
    )
}

pub fn handle(cmd: ImageCmd, options: GlobalOptions) -> anyhow::Result<()> {
    match cmd {
        ImageCmd::Render {
            target_serial_sha256,
            simulation_file,
            draft,
            render,
            input,
            output,
        } => handle_render(
            options,
            RenderRequest {
                target_serial_sha256,
                simulation_file,
                draft,
                render,
                input,
                output,
            },
        ),
        ImageCmd::Recover {
            target_serial_sha256,
            handle,
            output,
            delete_after_save,
        } => handle_recover(
            options,
            target_serial_sha256,
            handle,
            output,
            delete_after_save,
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs, io};

    use anyhow::anyhow;
    use fujicli::features::render::RenderSaveError;
    use tempfile::tempdir;

    use super::{Output, RenderOutput, handle_recover, save_rendered_object};
    use crate::cli::GlobalOptions;

    #[derive(Default)]
    struct FailingCommitOutput {
        bytes: Vec<u8>,
    }

    impl io::Write for FailingCommitOutput {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl RenderOutput for FailingCommitOutput {
        fn commit(self) -> anyhow::Result<()> {
            Err(anyhow!("simulated local output commit failure"))
        }
    }

    #[test]
    fn saves_rendered_image_before_returning_camera_cleanup_failure() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("rendered.jpg");
        let rendered = b"rendered JPEG".to_vec();
        let output = Output::Path(destination.clone()).begin_write()?;

        let error = save_rendered_object(output, 42, &rendered, None, true, |_| {
            Err(anyhow!("simulated camera cleanup failure"))
        })
        .expect_err("camera cleanup failure must keep the command unsuccessful");

        assert!(
            destination.exists(),
            "successfully fetched JPEG must be saved despite cleanup failure"
        );
        assert_eq!(fs::read(destination)?, rendered);
        assert!(
            error
                .to_string()
                .contains("rendered JPEG was saved, but camera cleanup failed"),
            "unexpected cleanup error: {error:#}"
        );
        Ok(())
    }

    #[test]
    fn does_not_delete_camera_object_when_local_output_commit_fails() {
        let rendered = b"rendered JPEG".to_vec();
        let output = FailingCommitOutput::default();
        let cleanup_called = Cell::new(false);

        let error = save_rendered_object(
            output,
            42,
            &rendered,
            Some(anyhow!("simulated profile restore failure")),
            true,
            |_| {
                cleanup_called.set(true);
                Ok(())
            },
        )
        .expect_err("failed output commit must retain the camera object");

        assert!(!cleanup_called.get());
        assert!(error.to_string().contains("camera object 42 was retained"));
        let save = error
            .downcast_ref::<RenderSaveError>()
            .expect("save failure must retain a typed outcome");
        assert_eq!(save.handle(), 42);
        assert_eq!(
            save.profile_restore_error().map(ToString::to_string),
            Some("simulated profile restore failure".to_owned())
        );
    }

    #[test]
    fn recovery_keeps_camera_object_by_default_after_file_commit() -> anyhow::Result<()> {
        let directory = tempdir()?;
        let destination = directory.path().join("recovered.jpg");
        let output = Output::Path(destination.clone()).begin_write()?;
        let cleanup_called = Cell::new(false);

        save_rendered_object(output, 42, b"rendered JPEG", None, false, |_| {
            cleanup_called.set(true);
            Ok(())
        })?;

        assert_eq!(fs::read(destination)?, b"rendered JPEG");
        assert!(!cleanup_called.get());
        Ok(())
    }

    #[test]
    fn recovery_forbids_delete_after_stdout_before_camera_access() {
        let error = handle_recover(
            GlobalOptions {
                json: false,
                verbose: 0,
                device: None,
                emulate: None,
            },
            None,
            42,
            Output::Stdout,
            true,
        )
        .expect_err("stdout cannot provide a durable receipt for camera deletion");

        assert!(error.to_string().contains("requires a file output"));
    }
}
