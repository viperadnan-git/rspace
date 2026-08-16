//! Icon glyph resolution: file/folder icons and the rclone-backend brand map.

use gpui::{prelude::*, rgb, svg};

use crate::theme::*;

use super::rem;

pub fn file_icon(is_dir: bool) -> impl IntoElement {
    let path = if is_dir { "icons/folder.svg" } else { "icons/file.svg" };
    svg().path(path).size(rem(15.0)).flex_shrink_0().text_color(rgb(FG_MUTED))
}

/// Glyph for an rclone backend type, keyed by `RemoteInfo::kind`. Brand icons
/// where available, else a category icon; unknown/new backends fall back to a
/// generic cloud. Add a provider by giving it an arm here.
pub fn remote_icon(kind: &str) -> &'static str {
    match kind {
        "drive" => "icons/drive.svg",
        "dropbox" => "icons/dropbox.svg",
        "googlecloudstorage" => "icons/gcs.svg",
        "b2" => "icons/b2.svg",
        "box" => "icons/box.svg",
        "mega" => "icons/mega.svg",
        "swift" => "icons/swift.svg",
        "yandex" => "icons/yandex.svg",
        "protondrive" => "icons/protondrive.svg",
        "iclouddrive" => "icons/icloud.svg",
        "onedrive" => "icons/onedrive.svg",
        "s3" => "icons/s3.svg",
        "azureblob" | "azurefiles" => "icons/azureblob.svg",
        "googlephotos" => "icons/googlephotos.svg",
        "internetarchive" => "icons/internetarchive.svg",
        "zoho" => "icons/zoho.svg",
        "seafile" => "icons/seafile.svg",
        "mailru" => "icons/mailru.svg",
        "sharefile" => "icons/sharefile.svg",
        "smb" => "icons/smb.svg",
        "pixeldrain" => "icons/image.svg",

        // Local disk / in-process.
        "local" => "icons/hard_drive.svg",
        "memory" => "icons/memory.svg",
        "cache" => "icons/cache.svg",
        // Network protocols (generic WebDAV is just a protocol, not Nextcloud).
        "sftp" | "ftp" | "http" | "hdfs" | "nfs" | "webdav" => "icons/server.svg",
        "nextcloud" => "icons/nextcloud.svg",
        "owncloud" => "icons/owncloud.svg",
        "qingstor" | "oracleobjectstorage" | "storj" | "sia" | "netstorage" => "icons/database.svg",
        "crypt" => "icons/lock.svg",
        "hasher" => "icons/hasher.svg",
        "compress" => "icons/compress.svg",
        "chunker" => "icons/chunker.svg",
        "union" | "combine" => "icons/union.svg",
        "alias" => "icons/alias.svg",

        _ => "icons/cloud.svg",
    }
}
