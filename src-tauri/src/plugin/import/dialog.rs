use std::{future::Future, pin::Pin};

use tauri::WebviewWindow;
use tauri_plugin_dialog::{DialogExt, FilePath};

use crate::error::{AppError, AppResult};
use crate::plugin::import::{LocalManifestSelector, SelectedManifestSource};

pub(crate) struct NativeLocalManifestSelector {
    window: WebviewWindow,
}

impl NativeLocalManifestSelector {
    pub(crate) fn new(window: WebviewWindow) -> Self {
        Self { window }
    }
}

impl LocalManifestSelector for NativeLocalManifestSelector {
    fn select(
        &self,
    ) -> Pin<Box<dyn Future<Output = AppResult<SelectedManifestSource>> + Send + '_>> {
        Box::pin(async move {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            self.window
                .dialog()
                .file()
                .set_parent(&self.window)
                .add_filter("JSON", &["json"])
                .set_title("选择插件清单")
                .pick_file(move |choice| {
                    let _ = sender.send(choice);
                });
            receive_native_choice(receiver).await
        })
    }
}

async fn receive_native_choice(
    receiver: tokio::sync::oneshot::Receiver<Option<FilePath>>,
) -> AppResult<SelectedManifestSource> {
    let choice = receiver.await.map_err(|_| dialog_unavailable())?;
    map_native_choice(choice)
}

fn map_native_choice(choice: Option<FilePath>) -> AppResult<SelectedManifestSource> {
    match choice {
        None => Ok(SelectedManifestSource::Cancelled),
        Some(FilePath::Path(path)) => Ok(SelectedManifestSource::Selected(path)),
        Some(FilePath::Url(_)) => Err(source_rejected()),
    }
}

fn dialog_unavailable() -> AppError {
    AppError::Plugin {
        code: "plugin_import_dialog_unavailable",
        message: "无法打开文件选择器，请重试。",
        diagnostic: None,
    }
}

fn source_rejected() -> AppError {
    AppError::Plugin {
        code: "plugin_import_source_rejected",
        message: "无法安全读取所选文件，请选择普通本地 JSON 文件。",
        diagnostic: None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tauri_plugin_dialog::FilePath;

    use super::{map_native_choice, receive_native_choice};
    use crate::error::AppError;
    use crate::plugin::import::SelectedManifestSource;

    #[test]
    fn native_none_is_cancelled_not_error() {
        assert!(matches!(
            map_native_choice(None).unwrap(),
            SelectedManifestSource::Cancelled
        ));
    }

    #[test]
    fn native_uri_is_source_rejected() {
        let error = map_native_choice(Some(FilePath::Url(
            "file:///tmp/manifest.json".parse().unwrap(),
        )))
        .err()
        .expect("URI must be rejected without conversion");

        assert_plugin_error(
            error,
            "plugin_import_source_rejected",
            "无法安全读取所选文件，请选择普通本地 JSON 文件。",
        );
    }

    #[tokio::test]
    async fn selector_channel_close_is_dialog_unavailable() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        drop(sender);

        let error = receive_native_choice(receiver)
            .await
            .err()
            .expect("closed channel must be unavailable");
        assert_plugin_error(
            error,
            "plugin_import_dialog_unavailable",
            "无法打开文件选择器，请重试。",
        );
    }

    #[test]
    fn native_path_is_forwarded_unchanged() {
        let path = PathBuf::from(r"C:\selected\..\manifest.json");

        let selected = map_native_choice(Some(FilePath::Path(path.clone()))).unwrap();
        let SelectedManifestSource::Selected(actual) = selected else {
            panic!("native path must remain selected");
        };
        assert_eq!(actual, path);
    }

    fn assert_plugin_error(error: AppError, code: &'static str, message: &'static str) {
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({"code": code, "message": message})
        );
        match error {
            AppError::Plugin {
                code: actual_code,
                message: actual_message,
                diagnostic,
            } => {
                assert_eq!(actual_code, code);
                assert_eq!(actual_message, message);
                assert!(diagnostic.is_none());
            }
            other => panic!("expected plugin error, got {other:?}"),
        }
    }
}
