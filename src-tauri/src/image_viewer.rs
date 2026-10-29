use std::{collections::HashSet, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicU64, Ordering}}};
use serde::Serialize;
use tauri::{Emitter, Manager};

#[derive(Default, Clone)]
pub(crate) struct ViewerState(Arc<Mutex<Option<Session>>>);
#[derive(Clone)]
struct Session { root: PathBuf, view: ViewerSession }
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ViewerSession { id: u64, images: Vec<ViewerImage>, index: usize }
#[derive(Clone, Serialize)]
struct ViewerImage { path: String, name: String }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TrashedImage { session: ViewerSession, path: String, name: String, warning: Option<String> }
fn next_session_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn validate_image(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| format!("Image is unavailable: {error}"))?;
    if !metadata.is_file() || !crate::image_file(path) { return Err("Select a supported, regular image file.".into()); }
    let resolved = path.canonicalize().map_err(|error| error.to_string())?;
    if !resolved.starts_with(root) { return Err("The image is outside this viewer's workspace.".into()); }
    Ok(resolved)
}
fn make_session(root: PathBuf, paths: Vec<String>, selected: String) -> Result<Session, String> {
    if paths.is_empty() || paths.len() > 100_000 { return Err("Choose between 1 and 100,000 images to view.".into()); }
    let selected = validate_image(&root, Path::new(&selected))?;
    let mut images = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        let resolved = validate_image(&root, Path::new(&path))?;
        if seen.insert(resolved.clone()) {
            images.push(ViewerImage { name: resolved.file_name().ok_or("Cannot read image name")?.to_string_lossy().into_owned(), path: resolved.to_string_lossy().into_owned() });
        }
    }
    let index = images.iter().position(|image| Path::new(&image.path) == selected).ok_or("The selected image is no longer in these results. Refresh and try again.")?;
    Ok(Session { root, view: ViewerSession { id: next_session_id(), images, index } })
}
#[tauri::command]
pub(crate) async fn begin_image_viewer(paths: Vec<String>, selected: String, state: tauri::State<'_, crate::AppState>, viewer: tauri::State<'_, ViewerState>, app: tauri::AppHandle) -> Result<(), String> {
    let root = crate::selected_root(&state)?;
    let viewer = viewer.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let session = make_session(root, paths, selected)?;
        let view = session.view.clone();
        *viewer.0.lock().map_err(|_| "Image viewer is unavailable")? = Some(session);
        // A new window reads the session on startup; an existing window receives this update.
        app.emit_to("image-viewer", "framewise:image-viewer-session", &view).map_err(|error| error.to_string())?;
        Ok(())
    }).await.map_err(|error| error.to_string())?
}
#[tauri::command]
pub(crate) fn image_viewer_session(viewer: tauri::State<'_, ViewerState>) -> Result<ViewerSession, String> {
    viewer.0.lock().map_err(|_| "Image viewer is unavailable")?.as_ref().map(|session| session.view.clone()).ok_or_else(|| "No image has been opened. Open an image from the inspector.".into())
}
fn session_image(session: &Session, session_id: u64, index: usize) -> Result<PathBuf, String> {
    if session.view.id != session_id { return Err("A newer image selection was opened. Please try again.".into()); }
    let image = session.view.images.get(index).ok_or("This image is not in the viewer's results.")?;
    validate_image(&session.root, Path::new(&image.path))
}
#[tauri::command]
pub(crate) async fn prepare_viewer_image(session_id: u64, index: usize, viewer: tauri::State<'_, ViewerState>, app: tauri::AppHandle) -> Result<crate::ImagePreview, String> {
    let session = viewer.0.lock().map_err(|_| "Image viewer is unavailable")?.clone().ok_or("No image has been opened.")?;
    tauri::async_runtime::spawn_blocking(move || {
        let image = session_image(&session, session_id, index)?;
        app.asset_protocol_scope().allow_file(&image).map_err(|error| error.to_string())?;
        Ok(crate::ImagePreview { version: crate::editor::signature(&image)?, image_path: image.to_string_lossy().into_owned() })
    }).await.map_err(|error| error.to_string())?
}

fn trash_image(session: &mut Session, session_id: u64, index: usize, version: &str, delete: impl FnOnce(&Path) -> Result<Option<String>, String>) -> Result<TrashedImage, String> {
    let image = session_image(session, session_id, index)?;
    if crate::editor::signature(&image)? != version {
        return Err("This image has changed since it was displayed. Reload it before deleting. No files were deleted.".into());
    }
    let warning = delete(&image)?;
    if !matches!(std::fs::symlink_metadata(&image), Err(error) if error.kind() == std::io::ErrorKind::NotFound) {
        return Err(warning.unwrap_or_else(|| "The image could not be moved to Trash.".into()));
    }
    let removed = session.view.images.remove(index);
    session.view.id = next_session_id();
    session.view.index = index.min(session.view.images.len().saturating_sub(1));
    Ok(TrashedImage { session: session.view.clone(), path: removed.path, name: removed.name, warning })
}

#[tauri::command]
pub(crate) async fn trash_viewer_image(session_id: u64, index: usize, version: String, viewer: tauri::State<'_, ViewerState>, app: tauri::AppHandle) -> Result<TrashedImage, String> {
    let viewer = viewer.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = {
            let mut slot = viewer.0.lock().map_err(|_| "Image viewer is unavailable")?;
            let session = slot.as_mut().ok_or("No image has been opened.")?;
            trash_image(session, session_id, index, &version, |image| {
                let operation = crate::catalog_operations::Operation::begin(crate::catalog_operations::Action::Trash, vec![image.to_owned()], vec![None])?;
                let deletion = trash::delete(image).map_err(|error| format!("Could not move image to Trash: {error}"));
                let finished = operation.finish();
                let warnings: Vec<_> = [deletion.err(), finished.err()].into_iter().flatten().collect();
                Ok(if warnings.is_empty() { None } else { Some(warnings.join(" ")) })
            })?
        };
        crate::forget_video_preview(Path::new(&result.path), None);
        if let Err(error) = app.emit_to("main", "framewise:image-trashed", serde_json::json!({ "path": result.path, "name": result.name, "warning": result.warning })) {
            log::warn!("Image was trashed but the main window could not be notified: {error}");
        }
        Ok(result)
    }).await.map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::tests::Fixture;
    #[test]
    fn trash_checks_displayed_identity_and_advances_only_after_success() {
        let fixture = Fixture::new(); let root = fixture.folder.canonicalize().unwrap();
        let paths: Vec<_> = ["a.png","b.png","c.png"].iter().map(|name| root.join(name)).collect();
        for path in &paths { std::fs::write(path,b"image").unwrap(); }
        let mut session = make_session(root.clone(), paths.iter().map(|path| path.to_string_lossy().into_owned()).collect(), paths[1].to_string_lossy().into_owned()).unwrap();
        let version = crate::editor::signature(&paths[1]).unwrap(); let id = session.view.id;
        assert!(trash_image(&mut session,id+1,1,&version, |_| panic!("Stale session must not delete")).is_err());
        std::fs::write(&paths[1],b"replacement image contents").unwrap();
        assert!(trash_image(&mut session,id,1,&version, |_| panic!("A replaced image must not be deleted")).is_err());
        std::fs::write(&paths[1],b"image").unwrap();
        let version = crate::editor::signature(&paths[1]).unwrap();
        assert!(trash_image(&mut session,id,1,"old version", |_| panic!("Changed image must not delete")).is_err());
        assert!(trash_image(&mut session,id,1,&version, |_| Err("simulated Trash failure".into())).is_err());
        assert!(trash_image(&mut session,id,1,&version, |_| Ok(None)).is_err());
        assert_eq!(session.view.images.len(),3); assert!(paths.iter().all(|path| path.exists()));
        let simulated_trash = root.join("simulated-trash"); std::fs::create_dir(&simulated_trash).unwrap();
        let move_file = |path: &Path| { std::fs::rename(path, simulated_trash.join(path.file_name().unwrap())).map_err(|error| error.to_string())?; Ok(None) };
        let result = trash_image(&mut session,id,1,&version,move_file).unwrap();
        assert_eq!(result.session.images.len(),2); assert_eq!(result.session.index,1);
        assert_eq!(result.session.images[1].name,"c.png"); assert_ne!(result.session.id,id);
        assert!(paths[0].exists() && !paths[1].exists() && paths[2].exists());
        let id = session.view.id; let version = crate::editor::signature(&paths[2]).unwrap();
        let result = trash_image(&mut session,id,1,&version,move_file).unwrap();
        assert_eq!(result.session.index,0); assert_eq!(result.session.images[0].name,"a.png");
        let id = session.view.id; let version = crate::editor::signature(&paths[0]).unwrap();
        assert!(trash_image(&mut session,id,0,&version,move_file).unwrap().session.images.is_empty());
        assert_eq!(session.view.index,0);
    }
    #[test]
    fn sessions_keep_filtered_order_selected_image_and_original_workspace() {
        let fixture = Fixture::new(); let root = fixture.folder.canonicalize().unwrap();
        let first = root.join("first.png"); let second = root.join("second.jpg");
        for path in [&first,&second] { std::fs::write(path,b"image").unwrap(); }
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let session = make_session(root.clone(), vec![text(&second),text(&first),text(&second)], text(&first)).unwrap();
        assert_eq!(session.view.index,1); assert_eq!(session.view.images.len(),2);
        assert_eq!(session_image(&session,session.view.id,0).unwrap(),second);
        assert!(session_image(&session,session.view.id + 1,0).is_err());
        assert!(session_image(&session,session.view.id,2).is_err());
        assert!(make_session(root.clone(),vec![text(&second)],text(&first)).is_err());
        assert!(make_session(root.join("elsewhere"),vec![text(&second)],text(&second)).is_err());
        std::fs::remove_file(&first).unwrap();
        assert!(session_image(&session,session.view.id,1).is_err());
        assert!(session_image(&session,session.view.id,0).is_ok());
        assert!(make_session(root.clone(),vec![text(&root)],text(&root)).is_err());
        let video = root.join("clip.mp4"); std::fs::write(&video,b"video").unwrap();
        assert!(make_session(root.clone(),vec![text(&video)],text(&video)).is_err());
        #[cfg(unix)] {
            let link = root.join("link.png"); std::os::unix::fs::symlink(&second,&link).unwrap();
            assert!(make_session(root,vec![text(&link)],text(&link)).is_err());
        }
    }
}
