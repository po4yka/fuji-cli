use std::{fs, process::Command};

fn external_check(source: &str) -> bool {
    let fixture = tempfile::tempdir().expect("temporary external crate must be created");
    let source_path = fixture.path().join("main.rs");
    fs::write(&source_path, source).expect("fixture source must be written");

    let deps = std::env::current_exe()
        .expect("test executable path must be available")
        .parent()
        .expect("test executable must have a parent directory")
        .to_path_buf();
    let mut rlibs = fs::read_dir(&deps)
        .expect("test dependency directory must be readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("libfujicli-") && name.ends_with(".rlib"))
        })
        .collect::<Vec<_>>();
    rlibs.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .expect("fujicli rlib metadata must be readable")
    });
    let fujicli = rlibs
        .pop()
        .expect("the newest fujicli rlib must be available to the external fixture");

    Command::new("rustc")
        .arg("--edition")
        .arg("2024")
        .arg("--emit")
        .arg("metadata")
        .arg("--out-dir")
        .arg(fixture.path())
        .arg("--extern")
        .arg(format!("fujicli={}", fujicli.display()))
        .arg("-L")
        .arg(format!("dependency={}", deps.display()))
        .arg(&source_path)
        .status()
        .expect("external fixture rustc check must start")
        .success()
}

#[test]
fn external_consumers_can_name_the_safe_high_level_api() {
    assert!(
        external_check(
            "use fujicli::{Camera, CameraInfo, CameraMode};\nuse fujicli::features::{backup::BackupArtifact, render::RenderOutcome, simulation::Simulation};\n\nfn main() {\n    let _: Option<Camera> = None;\n    let _: Option<CameraMode> = None;\n    let _: Option<Box<dyn CameraInfo>> = None;\n    let _: Option<BackupArtifact> = None;\n    let _: Option<RenderOutcome> = None;\n    let _: Option<Box<dyn Simulation>> = None;\n}\n"
        ),
        "the public API fixture itself must compile before negative assertions are trusted"
    );
}

#[test]
fn external_consumers_cannot_name_the_ptp_transport() {
    for (surface, source) in [
        (
            "PTP transport",
            "use fujicli::ptp::Ptp;\n\nfn main() {\n    let _: Option<Ptp> = None;\n}\n",
        ),
        (
            "camera implementation SPI",
            "use fujicli::features::base::CameraBase;\n\nfn main() {}\n",
        ),
        (
            "simulation manager SPI",
            "use fujicli::features::simulation::CameraSimulationManager;\n\nfn main() {}\n",
        ),
        (
            "render manager SPI",
            "use fujicli::features::render::CameraRenderManager;\n\nfn main() {}\n",
        ),
        (
            "raw simulation pull",
            "use fujicli::features::simulation::Simulation;\n\nfn denied<T: Simulation>() {\n    let _ = T::try_pull;\n}\n\nfn main() {}\n",
        ),
        (
            "camera transport field",
            "fn denied(camera: &fujicli::Camera) {\n    let _ = &camera.ptp;\n}\n\nfn main() {}\n",
        ),
    ] {
        assert!(
            !external_check(source),
            "the ordinary public library API must not expose {surface}"
        );
    }
}
