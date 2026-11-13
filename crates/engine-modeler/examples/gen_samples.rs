//! Записать образцы моделей в `assets/models/*.pmodel`.
//!
//! ```text
//! cargo run -p engine-modeler --example gen_samples [каталог]
//! ```
//!
//! Модели собираются тем же API, что и кнопки Piska Modeler (`engine_modeler::samples`).

fn main() {
    let dir = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models"));
    for (name, doc) in engine_modeler::samples::all() {
        let path = dir.join(format!("{name}.{}", engine_modeler::format::EXTENSION));
        match engine_modeler::format::save(&doc, &path) {
            Ok(()) => println!(
                "{}: {} elements, {} triangles",
                path.display(),
                doc.element_count(),
                engine_modeler::bake(&doc).triangle_count()
            ),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
}
