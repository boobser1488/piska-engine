//! Свои шейдеры в редакторе с настоящим вводом egui (без окна и GPU): Create → Shader
//! создаёт файл из шаблона и открывает его во вкладке Code; опечатка — волна на нужной
//! строке и строка Error List, исправление убирает их; инспектор материала показывает
//! параметры шейдера виджетами, правка пишется в `.material`.

use engine_assets::{AssetDatabase, ShaderKind};
use engine_core::prelude::App;

use crate::code_tests::{open_and_focus, temp_dir};
use crate::state::{Action, Editor, EditorConfig};
use crate::theme;

fn editor_on(dir: &std::path::Path) -> Editor {
    let mut app = App::new();
    app.world.resources.insert(AssetDatabase::open(dir).expect("assets open"));
    Editor::new(app, EditorConfig::default()).expect("the editor starts")
}

#[test]
fn create_shader_check_it_while_typing_and_edit_its_parameters_in_the_inspector() {
    let dir = temp_dir("shaders");
    std::fs::create_dir_all(dir.join("shaders")).unwrap();
    let mut editor = editor_on(&dir);

    // Create → Shader → Unlit: файл из шаблона, открыт во вкладке Code.
    editor.apply(Action::CreateShader { folder: "shaders".to_string(), kind: ShaderKind::Unlit });
    let path = dir.join("shaders/New Unlit Shader.shader");
    assert!(path.is_file(), "the shader file is created");
    let template = engine_render::shaders::template(ShaderKind::Unlit);
    let (open, text, _) = editor.code_document().expect("the new shader is open in the Code tab");
    assert_eq!(open, path.as_path());
    assert_eq!(text.replace("\r\n", "\n"), template);

    let mut hands = open_and_focus(&mut editor, &path);
    hands.wait(&mut editor, 0.5);
    assert_eq!(editor.code_problem_counts(), (0, 0, 0), "the template has no problems");

    // Опечатка в имени параметра: ошибка на строке пользователя с «did you mean».
    let line = template.lines().position(|l| l.contains("params.speed")).unwrap() + 1;
    editor.set_code_text(&template.replace("params.speed", "params.sped"));
    hands.wait(&mut editor, 0.5);
    assert_eq!(editor.code_problem_counts().0, 1, "one error after the pause");
    hands.frame(&mut editor, vec![]);
    assert!(hands.squiggles(theme::ERROR) >= 1, "a red wavy underline is painted");
    let rows = editor.error_list_rows();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert!(rows[0].contains(&format!("New Unlit Shader.shader:{line}:")), "{rows:?}");
    let entry = &crate::code_checks::entries(&editor.code)[0].diagnostic;
    assert!(entry.notes.iter().any(|n| n.contains("did you mean `speed`")), "{entry:?}");
    assert_eq!(entry.fixes.first().map(|f| f.edits[0].text.as_str()), Some("speed"));

    // Исправили — волна и строка пропали.
    editor.set_code_text(template);
    hands.wait(&mut editor, 0.5);
    assert_eq!(editor.code_problem_counts(), (0, 0, 0));
    assert_eq!(hands.squiggles(theme::ERROR), 0);
    editor.apply(Action::SaveCode);

    // Материал с этим шейдером: инспектор показывает параметры шейдера.
    editor.apply(Action::CreateMaterial { folder: "shaders".to_string() });
    let shader = editor.app.world.resources.get::<AssetDatabase>().unwrap().guid_of("shaders/New Unlit Shader.shader").unwrap();
    assert!(editor.edit_material(|m| m.shader = engine_assets::AssetHandle::from_guid(shader)));
    hands.wait(&mut editor, 0.1);
    let texts = hands.texts();
    for label in ["SHADER PARAMETERS", "Color A", "Color B", "Speed"] {
        assert!(texts.iter().any(|t| t == label), "`{label}` is shown: {texts:?}");
    }
    // Правка параметра (тот же путь, что у виджета) пишется в файл материала.
    assert!(editor.edit_material(|m| m.shader_params.set("speed", 2.5)));
    let file = std::fs::read_to_string(dir.join("shaders/New Material.material")).unwrap();
    assert!(file.contains("\"speed\": Float(2.5)"), "{file}");
    let _ = std::fs::remove_dir_all(&dir);
}
