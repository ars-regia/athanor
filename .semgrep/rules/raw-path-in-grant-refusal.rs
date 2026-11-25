fn opened<T>(paths: &[PathBuf], open: fn(&Path) -> io::Result<T>) -> io::Result<Vec<T>> {
    paths
        .iter()
        .filter_map(|path| match open(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => Some(Err(io::Error::new(
                error.kind(),
                // ruleid: raw-path-in-grant-refusal
                format!("{} is not granted: {error}", path.display()),
            ))),
            Ok(fd) => Some(Ok(fd)),
        })
        .collect()
}

fn confine(runtime_dir: &Path) -> Result<(), Box<dyn Error>> {
    let runtime_dir = dirs::own_dir(runtime_dir)
        // ruleid: raw-path-in-grant-refusal
        .map_err(|error| format!("{} is not granted: {error}", runtime_dir.display()))?;
    Ok(())
}

fn logged(dir: &Path, error: io::Error) {
    // ruleid: raw-path-in-grant-refusal
    tracing::warn!(%error, path = %dir.display(), "not granted: it fails the check");
}

fn led(dir: &Path, error: io::Error, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    // ruleid: raw-path-in-grant-refusal
    tracing::warn!(%error, "{} is not granted", dir.display());
    // ruleid: raw-path-in-grant-refusal
    write!(f, "{} is not granted", dir.display())
}

fn refused(path: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        // ok: raw-path-in-grant-refusal
        crate::text::line(
            &format!("{} is not granted: {why}", path.display()),
            usize::MAX,
        ),
    )
}

fn carried(path: &Path, error: io::Error) -> io::Error {
    // ok: raw-path-in-grant-refusal
    dirs::not_granted(path, error)
}

fn unrelated(path: &Path, error: io::Error) -> String {
    // ok: raw-path-in-grant-refusal
    format!("cleanup of {}: {error}", path.display())
}
