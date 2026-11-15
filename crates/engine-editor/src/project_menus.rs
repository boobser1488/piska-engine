//! Контекстные меню окна Project: элемент, пустое место, строка дерева. Все пункты кладут
//! [`Cmd`] в очередь — так одно и то же действие доступно и из меню, и с клавиатуры.

use crate::i18n::{tr_s};
use crate::icons::Icon;
use crate::project::{default_action, Place};
use crate::project_content::{Item, Kind};
use crate::project_ops::{Cmd, Create};
use crate::state::{Action, AssetEntry, Editor};
use crate::theme::{self, item as menu_item};

/// Что меню должно знать о редакторе (собирается раз в кадр).
pub(crate) struct MenuCx {
    pub can_paste: bool,
    pub cpp: bool,
    pub scene_object: bool,
    /// Открытый каталог.
    pub folder: String,
    pub favorites: Vec<String>,
    pub in_assets: bool,
    /// Программа Piska Modeler найдена (пункты «… in Modeler» активны).
    pub modeler: bool,
}

impl MenuCx {
    pub fn new(editor: &Editor) -> Self {
        Self {
            can_paste: editor.project.clipboard.as_ref().is_some_and(|c| !c.paths.is_empty()),
            cpp: editor.config.script_dir.is_some(),
            scene_object: editor.selection.is_some(),
            folder: if editor.project.place == Place::Code { String::new() } else { editor.project.folder.clone() },
            favorites: editor.prefs.project_ui.favorites.clone(),
            in_assets: editor.project.place == Place::Assets,
            modeler: crate::modeler_link::modeler_executable().is_some(),
        }
    }
}

/// Create ▸ … в каталог `folder`.
pub(crate) fn create_menu(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, cx: &MenuCx, folder: &str) {
    theme::submenu(ui, Icon::Plus, tr_s("Create"), |ui| create_items(ui, cmds, cx, folder));
}

/// Пункты Create: подменю контекстного меню и кнопка «Create ▾» на полосе окна.
pub(crate) fn create_items(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, cx: &MenuCx, folder: &str) {
    {
        let mut create = |kind: Create| cmds.push(Cmd::Create { kind, folder: folder.to_string() });
        if menu_item(tr_s("Folder")).icon(Icon::Folder).show(ui) {
            create(Create::Folder);
        }
        if menu_item(tr_s("Scene")).icon(Icon::Scene).show(ui) {
            create(Create::Scene);
        }
        if menu_item(tr_s("Prefab from Selection"))
            .icon(Icon::Prefab)
            .enabled_if(cx.scene_object, tr_s("Select an object in the Hierarchy first"))
            .show(ui)
        {
            create(Create::PrefabFromSelection);
        }
        if menu_item(tr_s("Material")).icon(Icon::Sphere).show(ui) {
            create(Create::Material);
        }
        theme::submenu(ui, Icon::Code, tr_s("Shader"), |ui| {
            let kinds = [
                (engine_assets::ShaderKind::Surface, tr_s("Surface Shader")),
                (engine_assets::ShaderKind::Unlit, tr_s("Unlit Shader")),
                (engine_assets::ShaderKind::Sprite, tr_s("Sprite Shader")),
                (engine_assets::ShaderKind::Post, tr_s("Post Effect")),
            ];
            for (kind, label) in kinds {
                if menu_item(label).icon(Icon::Code).show(ui) {
                    create(Create::Shader(kind));
                }
            }
        });
        theme::menu_separator(ui);
        if menu_item(tr_s("Rust Script…")).icon(Icon::Script).show(ui) {
            create(Create::RustScript);
        }
        if menu_item(tr_s("C++ Script…"))
            .icon(Icon::Script)
            .enabled_if(cx.cpp, "Start the editor with `EditorConfig::cpp_scripts(dir)`")
            .show(ui)
        {
            create(Create::CppScript);
        }
        if menu_item(tr_s("Scenario Script…")).icon(Icon::Script).show(ui) {
            create(Create::ScenarioScript);
        }
        theme::menu_separator(ui);
        if menu_item(tr_s("Model (Piska Modeler)")).icon(Icon::Cube).show(ui) {
            create(Create::Model);
        }
    }
}

fn clipboard_block(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, cx: &MenuCx, keys: &[String], paste_into: &str, duplicate: bool) {
    if menu_item(tr_s("Cut")).icon(Icon::Copy).shortcut("Ctrl+X").enabled_if(!keys.is_empty(), tr_s("Nothing is selected")).show(ui) {
        cmds.push(Cmd::Cut(keys.to_vec()));
    }
    if menu_item(tr_s("Copy")).icon(Icon::Copy).shortcut("Ctrl+C").enabled_if(!keys.is_empty(), tr_s("Nothing is selected")).show(ui) {
        cmds.push(Cmd::Copy(keys.to_vec()));
    }
    if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(cx.can_paste, tr_s("Copy or cut something first")).show(ui) {
        cmds.push(Cmd::Paste(paste_into.to_string()));
    }
    if duplicate && menu_item(tr_s("Duplicate")).icon(Icon::Copy).shortcut("Ctrl+D").show(ui) {
        cmds.push(Cmd::Duplicate(keys.to_vec()));
    }
}

/// Меню элемента содержимого. `keys` — выделение (или сам элемент, если он не выделен).
pub(crate) fn item_menu(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, cx: &MenuCx, item: &Item, keys: Vec<String>) {
    theme::menu_body(ui);
    if item.kind == Kind::Source {
        if menu_item(tr_s("Open in Code Tab")).icon(Icon::Code).show(ui) {
            cmds.push(Cmd::SourceOpen(item.disk.clone()));
        }
        let script = item.disk.extension().is_some_and(|e| matches!(e.to_str(), Some("rs" | "cpp" | "h" | "hpp")))
            && item.disk.file_name().is_some_and(|n| n != "mod.rs");
        if script {
            theme::menu_separator(ui);
            if menu_item(tr_s("Rename…")).icon(Icon::Text).show(ui) {
                cmds.push(Cmd::SourceRename(item.disk.clone()));
            }
            if menu_item(tr_s("Delete…")).icon(Icon::Trash).show(ui) {
                cmds.push(Cmd::SourceDelete(item.disk.clone()));
            }
        }
        return;
    }
    let single = keys.len() == 1;
    let is_folder = item.kind == Kind::Folder;
    let create_in = if single && is_folder { item.key.clone() } else { cx.folder.clone() };
    create_menu(ui, cmds, cx, &create_in);
    theme::menu_separator(ui);

    let entry = AssetEntry { guid: item.guid, path: item.key.clone(), importer: item.importer.clone() };
    let open_label = if !single || is_folder {
        Some(tr_s("Open"))
    } else {
        default_action(&entry, std::path::Path::new("")).map(|(label, _)| label)
    };
    if let Some(label) = open_label {
        if menu_item(label).icon(Icon::FolderOpen).shortcut("Enter").show(ui) {
            cmds.push(Cmd::Open(keys.clone()));
        }
    }
    if single && !is_folder && matches!(item.importer.as_str(), "Scene" | "Prefab") && menu_item(tr_s("Edit as Text")).icon(Icon::Code).show(ui) {
        cmds.push(Cmd::Act(Action::OpenCode(item.disk.clone())));
    }
    // Модель — править в Piska Modeler; чужой меш — править копию (новая .pmodel).
    if let Some(entry) = (single && !is_folder).then(|| crate::modeler_link::ModelerEntry::for_path(&item.disk)).flatten() {
        if menu_item(tr_s(entry.label()))
            .icon(Icon::Cube)
            .enabled_if(cx.modeler, tr_s(crate::modeler_link::MODELER_MISSING))
            .show(ui)
        {
            cmds.push(Cmd::Act(match entry {
                crate::modeler_link::ModelerEntry::Edit => Action::EditInModeler(item.disk.clone()),
                crate::modeler_link::ModelerEntry::EditCopy => Action::EditCopyInModeler(item.disk.clone()),
            }));
        }
    }
    if menu_item(tr_s("Show in File Manager")).icon(Icon::External).show(ui) {
        cmds.push(Cmd::Reveal(item.key.clone()));
    }
    theme::menu_separator(ui);

    let paste_into = if single && is_folder { item.key.as_str() } else { cx.folder.as_str() };
    clipboard_block(ui, cmds, cx, &keys, paste_into, true);
    theme::menu_separator(ui);
    if menu_item(tr_s("Rename")).icon(Icon::Text).shortcut("F2").enabled_if(single, tr_s("Select one item to rename")).show(ui) {
        cmds.push(Cmd::BeginRename(item.key.clone()));
    }
    if menu_item(tr_s("Delete")).icon(Icon::Trash).shortcut("Del").show(ui) {
        cmds.push(Cmd::Delete(keys.clone()));
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Copy Path")).icon(Icon::Copy).enabled_if(single, tr_s("Select one item")).show(ui) {
        cmds.push(Cmd::CopyPath(item.key.clone()));
    }
    if !is_folder && menu_item(tr_s("Copy GUID")).icon(Icon::Copy).enabled_if(single, tr_s("Select one asset")).show(ui) {
        cmds.push(Cmd::CopyGuid(item.key.clone()));
    }
    if is_folder && single {
        let pinned = cx.favorites.contains(&item.key);
        if pinned {
            if menu_item(tr_s("Remove from Favorites")).icon(Icon::Check).show(ui) {
                cmds.push(Cmd::Unpin(item.key.clone()));
            }
        } else if menu_item(tr_s("Add to Favorites")).icon(Icon::Check).show(ui) {
            cmds.push(Cmd::Pin(item.key.clone()));
        }
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Reimport")).icon(Icon::Refresh).show(ui) {
        cmds.push(Cmd::Reimport(keys.clone()));
    }
    if !is_folder
        && menu_item(tr_s("Find References In Scene"))
            .icon(Icon::Search)
            .enabled_if(single, tr_s("Select one asset"))
            .show(ui)
    {
        cmds.push(Cmd::FindReferences(item.key.clone()));
    }
    if menu_item(tr_s("Properties")).icon(Icon::Info).enabled_if(single, tr_s("Select one item")).show(ui) {
        cmds.push(Cmd::Properties);
    }
}

/// Меню пустого места каталога.
pub(crate) fn space_menu(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, cx: &MenuCx) {
    theme::menu_body(ui);
    if !cx.in_assets {
        if menu_item(tr_s("Refresh")).icon(Icon::Refresh).shortcut("Ctrl+R").show(ui) {
            cmds.push(Cmd::Refresh);
        }
        return;
    }
    create_menu(ui, cmds, cx, &cx.folder);
    theme::menu_separator(ui);
    if menu_item(tr_s("Import New Asset…")).icon(Icon::ArrowDown).show(ui) {
        cmds.push(Cmd::ImportDialog(cx.folder.clone()));
    }
    if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(cx.can_paste, tr_s("Copy or cut something first")).show(ui) {
        cmds.push(Cmd::Paste(cx.folder.clone()));
    }
    if menu_item(tr_s("Select All")).icon(Icon::Check).shortcut("Ctrl+A").show(ui) {
        cmds.push(Cmd::SelectAll);
    }
    theme::menu_separator(ui);
    if menu_item(tr_s("Show in File Manager")).icon(Icon::External).show(ui) {
        cmds.push(Cmd::Reveal(cx.folder.clone()));
    }
    if menu_item(tr_s("Refresh")).icon(Icon::Refresh).shortcut("Ctrl+R").show(ui) {
        cmds.push(Cmd::Refresh);
    }
}

/// Меню строки дерева каталогов.
pub(crate) fn tree_menu(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>, cx: &MenuCx, folder: &str) {
    theme::menu_body(ui);
    let root = folder.is_empty();
    create_menu(ui, cmds, cx, folder);
    theme::menu_separator(ui);
    if menu_item(tr_s("Open")).icon(Icon::FolderOpen).show(ui) {
        cmds.push(Cmd::Go(folder.to_string()));
    }
    if menu_item(tr_s("Show in File Manager")).icon(Icon::External).show(ui) {
        cmds.push(Cmd::Reveal(folder.to_string()));
    }
    if menu_item(tr_s("Import New Asset…")).icon(Icon::ArrowDown).show(ui) {
        cmds.push(Cmd::ImportDialog(folder.to_string()));
    }
    theme::menu_separator(ui);
    let keys = vec![folder.to_string()];
    if root {
        if menu_item(tr_s("Paste")).icon(Icon::ArrowDown).shortcut("Ctrl+V").enabled_if(cx.can_paste, tr_s("Copy or cut something first")).show(ui) {
            cmds.push(Cmd::Paste(String::new()));
        }
    } else {
        clipboard_block(ui, cmds, cx, &keys, folder, true);
        theme::menu_separator(ui);
        if menu_item(tr_s("Rename")).icon(Icon::Text).shortcut("F2").show(ui) {
            cmds.push(Cmd::BeginRename(folder.to_string()));
        }
        if menu_item(tr_s("Delete")).icon(Icon::Trash).shortcut("Del").show(ui) {
            cmds.push(Cmd::Delete(keys.clone()));
        }
        theme::menu_separator(ui);
        if menu_item(tr_s("Copy Path")).icon(Icon::Copy).show(ui) {
            cmds.push(Cmd::CopyPath(folder.to_string()));
        }
        if cx.favorites.iter().any(|f| f == folder) {
            if menu_item(tr_s("Remove from Favorites")).icon(Icon::Check).show(ui) {
                cmds.push(Cmd::Unpin(folder.to_string()));
            }
        } else if menu_item(tr_s("Add to Favorites")).icon(Icon::Check).show(ui) {
            cmds.push(Cmd::Pin(folder.to_string()));
        }
    }
    if menu_item(tr_s("Refresh")).icon(Icon::Refresh).shortcut("Ctrl+R").show(ui) {
        cmds.push(Cmd::Refresh);
    }
}
