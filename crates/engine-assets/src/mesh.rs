//! Меши: данные на CPU и импорт из OBJ и glTF.
//!
//! Импорт сводит файл к одному [`MeshData`]: все части и узлы склеиваются, матрицы
//! узлов glTF применяются к вершинам. Части с разными материалами остаются отдельными
//! диапазонами индексов ([`SubMesh`]) — каждый рисуется своим материалом; сами
//! материалы файла описывает [`MeshMaterial`].

use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;

#[cfg(feature = "gltf")]
use engine_core::math::Mat4;
use engine_core::math::Vec3;

use crate::material::AlphaMode;
use crate::AssetError;

/// Треугольный меш: по одному атрибуту на вершину и индексы троек.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// (0, 0) — левый верхний угол текстуры.
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    /// Касательные для карт нормалей: xyz — направление роста U, w — знак битангенса
    /// (`cross(normal, tangent) * w` смотрит «вверх» по картинке), как в glTF.
    /// Пусто — посчитать по UV ([`compute_tangents`](Self::compute_tangents)).
    pub tangents: Vec<[f32; 4]>,
    /// Части с разными материалами. Пусто — весь меш одна часть.
    pub submeshes: Vec<SubMesh>,
}

/// Часть меша со своим материалом: диапазон в [`MeshData::indices`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubMesh {
    pub indices: Range<u32>,
    /// Номер материала в списке материалов файла ([`MeshExtras::materials`]).
    pub material: Option<usize>,
    /// Имя материала части (`usemtl` в OBJ, имя материала glTF); пусто — без материала.
    pub name: String,
}

impl MeshData {
    /// Части меша: [`submeshes`](Self::submeshes) или одна часть на весь меш.
    pub fn parts(&self) -> Vec<SubMesh> {
        if self.submeshes.is_empty() {
            vec![SubMesh { indices: 0..self.indices.len() as u32, material: None, name: String::new() }]
        } else {
            self.submeshes.clone()
        }
    }

    /// Посчитать касательные по UV (накопление по треугольникам, затем ортогонализация
    /// к нормали). Там, где UV вырождены, берётся любая касательная, перпендикулярная нормали.
    pub fn compute_tangents(&mut self) {
        let count = self.positions.len();
        let mut tangent = vec![Vec3::ZERO; count];
        let mut bitangent = vec![Vec3::ZERO; count];
        for triangle in self.indices.chunks_exact(3) {
            let [a, b, c] = [triangle[0] as usize, triangle[1] as usize, triangle[2] as usize];
            if a >= count || b >= count || c >= count || self.uvs.len() < count {
                continue;
            }
            let p = |i: usize| Vec3::from_array(self.positions[i]);
            let (e1, e2) = (p(b) - p(a), p(c) - p(a));
            let (du1, dv1) = (self.uvs[b][0] - self.uvs[a][0], self.uvs[b][1] - self.uvs[a][1]);
            let (du2, dv2) = (self.uvs[c][0] - self.uvs[a][0], self.uvs[c][1] - self.uvs[a][1]);
            let det = du1 * dv2 - du2 * dv1;
            if det.abs() < 1e-12 {
                continue;
            }
            let r = 1.0 / det;
            let t = (e1 * dv2 - e2 * dv1) * r; // рост U
            let v = (e2 * du1 - e1 * du2) * r; // рост V (вниз по картинке)
            for index in [a, b, c] {
                tangent[index] += t;
                bitangent[index] += v;
            }
        }
        self.tangents = (0..count)
            .map(|i| {
                let n = self.normals.get(i).map_or(Vec3::Y, |n| Vec3::from_array(*n));
                let t = tangent[i] - n * n.dot(tangent[i]);
                let t = t.try_normalize().unwrap_or_else(|| n.any_orthonormal_vector());
                // «Вверх» по картинке — против роста V.
                let w = if n.cross(t).dot(-bitangent[i]) < 0.0 { -1.0 } else { 1.0 };
                [t.x, t.y, t.z, w]
            })
            .collect();
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Минимальный и максимальный углы ограничивающего параллелепипеда.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for position in &self.positions {
            let p = Vec3::from_array(*position);
            min = min.min(p);
            max = max.max(p);
        }
        if self.positions.is_empty() {
            (Vec3::ZERO, Vec3::ZERO)
        } else {
            (min, max)
        }
    }

    /// Пересчитать нормали как сглаженные: среднее нормалей граней, взвешенное площадью.
    pub fn compute_normals(&mut self) {
        let mut normals = vec![Vec3::ZERO; self.positions.len()];
        for triangle in self.indices.chunks_exact(3) {
            let [a, b, c] = [triangle[0] as usize, triangle[1] as usize, triangle[2] as usize];
            let (pa, pb, pc) = (
                Vec3::from_array(self.positions[a]),
                Vec3::from_array(self.positions[b]),
                Vec3::from_array(self.positions[c]),
            );
            let face = (pb - pa).cross(pc - pa); // длина = удвоенная площадь
            for index in [a, b, c] {
                normals[index] += face;
            }
        }
        self.normals = normals
            .into_iter()
            .map(|n| n.try_normalize().unwrap_or(Vec3::Y).to_array())
            .collect();
    }

    /// Дописать другой меш, преобразовав его вершины матрицей. Его части (или он
    /// целиком — одной частью с материалом `material`) становятся частями этого меша;
    /// соседние части с одним материалом сливаются.
    #[cfg(feature = "gltf")]
    fn append(&mut self, other: &MeshData, matrix: Mat4, material: Option<usize>, name: &str) {
        let base = self.positions.len() as u32;
        let first_index = self.indices.len() as u32;
        let normal_matrix = matrix.inverse().transpose();
        for position in &other.positions {
            self.positions
                .push(matrix.transform_point3(Vec3::from_array(*position)).to_array());
        }
        for normal in &other.normals {
            let n = normal_matrix.transform_vector3(Vec3::from_array(*normal));
            self.normals.push(n.try_normalize().unwrap_or(Vec3::Y).to_array());
        }
        // Касательные есть только у части частей — тогда их пересчитают для всего меша.
        if other.tangents.len() == other.positions.len() && self.tangents.len() == base as usize {
            let mirrored = matrix.determinant() < 0.0;
            for t in &other.tangents {
                let v = matrix.transform_vector3(Vec3::new(t[0], t[1], t[2])).try_normalize().unwrap_or(Vec3::X);
                self.tangents.push([v.x, v.y, v.z, if mirrored { -t[3] } else { t[3] }]);
            }
        } else {
            self.tangents.clear();
        }
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| base + i));
        let parts = if other.submeshes.is_empty() {
            vec![SubMesh { indices: 0..other.indices.len() as u32, material, name: name.to_string() }]
        } else {
            other.submeshes.clone()
        };
        for part in parts {
            let range = part.indices.start + first_index..part.indices.end + first_index;
            match self.submeshes.last_mut() {
                Some(last) if last.material == part.material && last.name == part.name && last.indices.end == range.start => {
                    last.indices.end = range.end;
                }
                _ => self.submeshes.push(SubMesh { indices: range, ..part }),
            }
        }
    }

    /// Проверить, что атрибуты согласованы, а индексы не выходят за вершины.
    fn validate(self, path: &Path) -> Result<Self, AssetError> {
        let fail = |reason: String| AssetError::Import {
            path: path.display().to_string(),
            reason,
        };
        let count = self.positions.len();
        if count == 0 || self.indices.is_empty() {
            return Err(fail("the file has no triangles".to_string()));
        }
        if self.normals.len() != count || self.uvs.len() != count {
            return Err(fail("vertex attributes have different lengths".to_string()));
        }
        if !self.indices.len().is_multiple_of(3) || self.indices.iter().any(|i| *i as usize >= count) {
            return Err(fail("triangle indices point outside the vertex list".to_string()));
        }
        let mut data = self;
        if !data.tangents.is_empty() && data.tangents.len() != count {
            data.tangents.clear();
        }
        // Одна часть без материала ничем не отличается от «частей нет».
        if data.submeshes.len() == 1 && data.submeshes[0].material.is_none() && data.submeshes[0].name.is_empty() {
            data.submeshes.clear();
        }
        let total = data.indices.len() as u32;
        if data.submeshes.iter().any(|s| s.indices.end > total || s.indices.start > s.indices.end) {
            return Err(fail("a sub-mesh points outside the index list".to_string()));
        }
        Ok(data)
    }
}

/// Загрузить меш по расширению файла: `.obj`, `.gltf`, `.glb`.
pub fn load_mesh(path: impl AsRef<Path>) -> Result<MeshData, AssetError> {
    let path = path.as_ref();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let import = |reason: String| AssetError::Import {
        path: path.display().to_string(),
        reason,
    };
    match extension.as_str() {
        "obj" => {
            let text = std::fs::read_to_string(path).map_err(|source| AssetError::Io {
                path: path.display().to_string(),
                source,
            })?;
            let mut mesh = parse_obj(&text).map_err(import)?;
            assign_obj_materials(&mut mesh, &text, &disk_resolver(path));
            mesh.validate(path)
        }
        "gltf" | "glb" => load_gltf(path).map_err(import)?.validate(path),
        "pmodel" => {
            let bytes = std::fs::read(path).map_err(|source| AssetError::Io {
                path: path.display().to_string(),
                source,
            })?;
            piska_model(&bytes).map_err(import)?.validate(path)
        }
        _ if crate::formats::is_model_format(&path.to_string_lossy()) => {
            let bytes = std::fs::read(path).map_err(|source| AssetError::Io {
                path: path.display().to_string(),
                source,
            })?;
            // Без базы ассетов — только файлы рядом с моделью и глубже, не выше.
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            let resolve = move |p: &str| (!p.starts_with("..")).then(|| std::fs::read(dir.join(p)).ok()).flatten();
            let name = path.to_string_lossy();
            crate::formats::load(&name, &bytes, &resolve, &Default::default()).map_err(import)?.validate(path)
        }
        other => Err(import(not_a_mesh(other))),
    }
}

/// Ошибка «не меш»: для известных чужих форматов (`.fbx`, `.blend`…) — что делать.
fn not_a_mesh(extension: &str) -> String {
    engine_model_formats::unsupported_message(&format!("x.{extension}")).unwrap_or_else(|| {
        format!("`.{extension}` is not a mesh format; supported: .obj, .gltf, .glb, .pmodel, .bbmodel, .model.json, .vox, .ply, .stl")
    })
}

/// Разобрать Wavefront OBJ: `v`, `vt`, `vn`, `f` (многоугольники режутся веером,
/// отрицательные индексы поддерживаются). Без нормалей в файле они вычисляются.
pub fn parse_obj(text: &str) -> Result<MeshData, String> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut mesh = MeshData::default();
    let mut seen: HashMap<(usize, Option<usize>, Option<usize>), u32> = HashMap::new();
    let mut missing_normals = false;

    for (line_index, line) in text.lines().enumerate() {
        let line_number = line_index + 1;
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        let mut numbers = |count: usize| -> Result<Vec<f32>, String> {
            let values: Vec<f32> = parts
                .by_ref()
                .take(count)
                .map(|text| {
                    text.parse::<f32>()
                        .map_err(|_| format!("line {line_number}: `{text}` is not a number"))
                })
                .collect::<Result<_, _>>()?;
            if values.len() < count {
                return Err(format!(
                    "line {line_number}: `{keyword}` needs {count} numbers, found {}",
                    values.len()
                ));
            }
            Ok(values)
        };
        match keyword {
            "v" => {
                let v = numbers(3)?;
                positions.push([v[0], v[1], v[2]]);
            }
            "vt" => {
                let v = numbers(2)?;
                uvs.push([v[0], 1.0 - v[1]]); // в OBJ (0, 0) — левый нижний угол
            }
            "vn" => {
                let v = numbers(3)?;
                normals.push([v[0], v[1], v[2]]);
            }
            "f" => {
                // Индексы в OBJ с единицы; отрицательные — от конца списка.
                let resolve = |text: &str, len: usize, what: &str| -> Result<usize, String> {
                    let index: i64 = text
                        .parse()
                        .map_err(|_| format!("line {line_number}: bad {what} index `{text}`"))?;
                    let resolved = if index < 0 { len as i64 + index } else { index - 1 };
                    if resolved < 0 || resolved as usize >= len {
                        return Err(format!(
                            "line {line_number}: {what} index {index} is out of range (the file has {len})"
                        ));
                    }
                    Ok(resolved as usize)
                };
                let mut corners = Vec::new();
                for corner in parts {
                    let mut fields = corner.split('/');
                    let position = resolve(fields.next().unwrap_or(""), positions.len(), "vertex")?;
                    let uv = match fields.next() {
                        Some(text) if !text.is_empty() => Some(resolve(text, uvs.len(), "texture")?),
                        _ => None,
                    };
                    let normal = match fields.next() {
                        Some(text) if !text.is_empty() => Some(resolve(text, normals.len(), "normal")?),
                        _ => None,
                    };
                    missing_normals |= normal.is_none();
                    let key = (position, uv, normal);
                    let index = *seen.entry(key).or_insert_with(|| {
                        mesh.positions.push(positions[position]);
                        mesh.uvs.push(uv.map_or([0.0, 0.0], |i| uvs[i]));
                        mesh.normals.push(normal.map_or([0.0, 1.0, 0.0], |i| normals[i]));
                        (mesh.positions.len() - 1) as u32
                    });
                    corners.push(index);
                }
                if corners.len() < 3 {
                    return Err(format!("line {line_number}: a face needs at least 3 vertices"));
                }
                for i in 1..corners.len() - 1 {
                    mesh.indices.extend([corners[0], corners[i], corners[i + 1]]);
                }
            }
            "usemtl" => {
                // Новая часть с этого места; номер материала проставит `assign_obj_materials`.
                let name = parts.next().unwrap_or("").to_string();
                let start = mesh.indices.len() as u32;
                if let Some(last) = mesh.submeshes.last_mut() {
                    last.indices.end = start;
                }
                mesh.submeshes.push(SubMesh { indices: start..start, material: None, name });
            }
            _ => {} // o, g, s, mtllib, комментарии
        }
    }
    if let Some(last) = mesh.submeshes.last_mut() {
        last.indices.end = mesh.indices.len() as u32;
    }
    if !mesh.submeshes.is_empty() && mesh.submeshes[0].indices.start > 0 {
        // Грани до первого `usemtl` — часть без материала.
        mesh.submeshes.insert(0, SubMesh { indices: 0..mesh.submeshes[0].indices.start, material: None, name: String::new() });
    }
    mesh.submeshes.retain(|s| !s.indices.is_empty());
    // Соседние части с одним материалом — одна часть.
    let mut merged: Vec<SubMesh> = Vec::with_capacity(mesh.submeshes.len());
    for part in std::mem::take(&mut mesh.submeshes) {
        match merged.last_mut() {
            Some(last) if last.name == part.name && last.indices.end == part.indices.start => last.indices.end = part.indices.end,
            _ => merged.push(part),
        }
    }
    mesh.submeshes = merged;
    if missing_normals {
        mesh.compute_normals();
    }
    Ok(mesh)
}

/// Проставить частям OBJ номера материалов из `.mtl` (по имени из `usemtl`).
fn assign_obj_materials(mesh: &mut MeshData, text: &str, resolve: &dyn Fn(&str) -> Option<Vec<u8>>) {
    if mesh.submeshes.is_empty() {
        return;
    }
    let (_, materials, _) = obj_extras(text, resolve);
    for part in &mut mesh.submeshes {
        part.material = materials.iter().position(|m| m.name == part.name);
    }
}

/// Чтение соседних файлов модели с диска.
fn disk_resolver(path: &Path) -> impl Fn(&str) -> Option<Vec<u8>> + '_ {
    move |uri: &str| std::fs::read(path.parent().unwrap_or(Path::new(".")).join(uri)).ok()
}

/// Сообщение, когда `.gltf` / `.glb` встретился в сборке без возможности `gltf`.
#[cfg(not(feature = "gltf"))]
const GLTF_OFF: &str = "glTF models need the `mesh-gltf` feature of the `engine` crate (`gltf` of `engine-assets`); \
                        enable it in Cargo.toml or convert the model to .obj";

#[cfg(not(feature = "gltf"))]
fn load_gltf(_path: &Path) -> Result<MeshData, String> {
    Err(GLTF_OFF.to_string())
}

#[cfg(feature = "gltf")]
fn load_gltf(path: &Path) -> Result<MeshData, String> {
    let (document, buffers, _images) = gltf::import(path).map_err(|e| e.to_string())?;
    gltf_mesh(&document, &buffers)
}

/// Загрузить меш из байтов файла — для паков ассетов, где файловой системы нет.
/// `name` — имя файла (по расширению выбирается формат), `resolve` — чтение соседних
/// файлов по относительному пути (внешние буферы `.gltf`).
pub fn load_mesh_bytes(
    name: &str,
    bytes: &[u8],
    resolve: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<MeshData, AssetError> {
    let path = Path::new(name);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let import = |reason: String| AssetError::Import {
        path: name.to_string(),
        reason,
    };
    match extension.as_str() {
        "obj" => {
            let text = std::str::from_utf8(bytes).map_err(|_| import("the file is not UTF-8 text".to_string()))?;
            let mut mesh = parse_obj(text).map_err(import)?;
            assign_obj_materials(&mut mesh, text, resolve);
            mesh.validate(path)
        }
        "gltf" | "glb" => gltf_from_slice(bytes, resolve).map_err(import)?.validate(path),
        "pmodel" => piska_model(bytes).map_err(import)?.validate(path),
        // Сторонние форматы; `.json` — если это модель Minecraft (её явно грузят как меш).
        _ if engine_model_formats::Format::detect(name, bytes).is_some() => {
            crate::formats::load(name, bytes, resolve, &Default::default()).map_err(import)?.validate(path)
        }
        other => Err(import(not_a_mesh(other))),
    }
}

/// Как [`load_mesh_bytes`], с настройками импорта из `.meta` (для сторонних форматов:
/// масштаб, ось вверх, отражение, части по текстурам, двусторонность).
pub fn load_mesh_bytes_with(
    name: &str,
    bytes: &[u8],
    resolve: &dyn Fn(&str) -> Option<Vec<u8>>,
    settings: &std::collections::BTreeMap<String, engine_core::reflect::Value>,
) -> Result<MeshData, AssetError> {
    if engine_model_formats::Format::detect(name, bytes).is_none() {
        return load_mesh_bytes(name, bytes, resolve);
    }
    let options = crate::formats::import_options(settings);
    crate::formats::load(name, bytes, resolve, &options)
        .map_err(|reason| AssetError::Import { path: name.to_string(), reason })?
        .validate(Path::new(name))
}

/// Раздел `baked` файла Piska Modeler (`.pmodel`): модель, уже собранная редактором
/// моделей в один меш в метрах, и её общий атлас (PNG в base64). Остальное содержимое
/// файла (редактируемый документ) движку не нужно.
#[derive(serde::Deserialize)]
struct PiskaModelFile {
    format: String,
    #[serde(default)]
    baked: Option<PiskaModelBaked>,
}

#[derive(serde::Deserialize)]
struct PiskaModelBaked {
    positions: Vec<f32>,
    normals: Vec<f32>,
    uvs: Vec<f32>,
    indices: Vec<u32>,
    #[serde(default)]
    texture: Option<String>,
}

fn piska_model_baked(bytes: &[u8]) -> Result<PiskaModelBaked, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "the file is not UTF-8 text".to_string())?;
    let file: PiskaModelFile = ron::from_str(text).map_err(|e| format!("not a valid .pmodel: {e}"))?;
    if file.format != "piska-model" {
        return Err(format!("format `{}` is not a Piska model", file.format));
    }
    file.baked
        .ok_or_else(|| "the model has no `baked` section; open and save it in Piska Modeler".to_string())
}

/// Меш модели Piska Modeler.
fn piska_model(bytes: &[u8]) -> Result<MeshData, String> {
    let baked = piska_model_baked(bytes)?;
    let triples = |v: &[f32]| v.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect::<Vec<_>>();
    Ok(MeshData {
        positions: triples(&baked.positions),
        normals: triples(&baked.normals),
        uvs: baked.uvs.chunks_exact(2).map(|c| [c[0], c[1]]).collect(),
        indices: baked.indices,
        ..MeshData::default()
    })
}

/// Атлас модели Piska Modeler (PNG).
pub(crate) fn piska_model_texture(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let texture = piska_model_baked(bytes)?.texture.ok_or("the model has no texture")?;
    let mut out = Vec::with_capacity(texture.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for c in texture.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return Err("the embedded texture is damaged".to_string()),
        };
        acc = acc << 6 | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

#[cfg(not(feature = "gltf"))]
fn gltf_from_slice(_bytes: &[u8], _resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<MeshData, String> {
    Err(GLTF_OFF.to_string())
}

#[cfg(feature = "gltf")]
fn gltf_from_slice(bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<MeshData, String> {
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let has_data_uri = document
        .buffers()
        .any(|b| matches!(b.source(), gltf::buffer::Source::Uri(uri) if uri.starts_with("data:")));
    let buffers = if has_data_uri {
        // Встроенные base64-буферы разбирает сам крейт gltf (внешних файлов тогда быть не должно).
        gltf::import_buffers(&document, None, blob).map_err(|e| e.to_string())?
    } else {
        let mut blob = blob;
        document
            .buffers()
            .map(|buffer| {
                let mut data = match buffer.source() {
                    gltf::buffer::Source::Bin => blob.take().ok_or("the .glb has no binary chunk")?,
                    gltf::buffer::Source::Uri(uri) => {
                        resolve(uri).ok_or_else(|| format!("buffer `{uri}` is not in the asset pack"))?
                    }
                };
                if data.len() < buffer.length() {
                    return Err(format!("buffer #{} is shorter than declared", buffer.index()));
                }
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                Ok(gltf::buffer::Data(data))
            })
            .collect::<Result<Vec<_>, String>>()?
    };
    gltf_mesh(&document, &buffers)
}

#[cfg(feature = "gltf")]
fn gltf_mesh(document: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Result<MeshData, String> {
    let mut mesh = MeshData::default();

    fn visit(
        node: gltf::Node,
        parent: Mat4,
        buffers: &[gltf::buffer::Data],
        out: &mut MeshData,
    ) -> Result<(), String> {
        let matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if let Some(source) = node.mesh() {
            for primitive in source.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue; // линии и точки не рисуем
                }
                let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|data| &data.0[..]));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let mut part = MeshData {
                    positions: positions.collect(),
                    ..MeshData::default()
                };
                let count = part.positions.len();
                part.indices = match reader.read_indices() {
                    Some(indices) => indices.into_u32().collect(),
                    None => (0..count as u32).collect(),
                };
                if part.indices.iter().any(|i| *i as usize >= count) {
                    return Err(format!(
                        "mesh #{} has indices outside its vertex list",
                        source.index()
                    ));
                }
                part.uvs = match reader.read_tex_coords(0) {
                    Some(uvs) => uvs.into_f32().collect(),
                    None => vec![[0.0, 0.0]; count],
                };
                match reader.read_normals() {
                    Some(normals) => part.normals = normals.collect(),
                    None => part.compute_normals(),
                }
                if let Some(tangents) = reader.read_tangents() {
                    part.tangents = tangents.collect();
                }
                if part.normals.len() != count || part.uvs.len() != count {
                    return Err("vertex attributes have different lengths".to_string());
                }
                let material = primitive.material();
                let name = material.name().map(str::to_string).unwrap_or_else(|| match material.index() {
                    Some(index) => format!("material {index}"),
                    None => String::new(),
                });
                out.append(&part, matrix, material.index(), &name);
            }
        }
        for child in node.children() {
            visit(child, matrix, buffers, out)?;
        }
        Ok(())
    }

    // Сцена по умолчанию; если её нет — все сцены файла.
    let scenes: Vec<gltf::Scene> = match document.default_scene() {
        Some(scene) => vec![scene],
        None => document.scenes().collect(),
    };
    for scene in scenes {
        for node in scene.nodes() {
            visit(node, Mat4::IDENTITY, buffers, &mut mesh)?;
        }
    }
    Ok(mesh)
}

/// Материал, описанный в файле модели. Пути текстур — относительно файла модели;
/// встроенные в `.glb` картинки не поддерживаются (`None`). В ассет [`Material`](crate::Material)
/// его превращает [`AssetDatabase::model_materials`](crate::AssetDatabase::model_materials).
#[derive(Clone, Debug, PartialEq)]
pub struct MeshMaterial {
    pub name: String,
    /// Базовый цвет RGBA, линейный (`Kd` в MTL, `baseColorFactor` в glTF).
    pub color: [f32; 4],
    /// Текстура базового цвета — путь относительно файла модели.
    pub texture: Option<String>,
    pub metallic: f32,
    pub roughness: f32,
    pub metallic_roughness_texture: Option<String>,
    pub normal_texture: Option<String>,
    pub normal_scale: f32,
    /// Свечение, линейное (`Ke` в MTL, `emissiveFactor` в glTF).
    pub emissive: [f32; 3],
    pub emissive_texture: Option<String>,
    pub occlusion_texture: Option<String>,
    pub alpha_mode: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
}

impl MeshMaterial {
    /// Материал без текстур с цветом `color` и параметрами по умолчанию.
    pub fn named(name: &str, color: [f32; 4]) -> Self {
        Self {
            name: name.to_string(),
            color,
            texture: None,
            metallic: 0.0,
            roughness: 0.6,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            emissive: [0.0; 3],
            emissive_texture: None,
            occlusion_texture: None,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: 0.5,
            double_sided: false,
        }
    }

    /// Все текстуры материала (пути относительно модели).
    pub fn texture_paths(&self) -> impl Iterator<Item = &String> {
        [&self.texture, &self.metallic_roughness_texture, &self.normal_texture, &self.emissive_texture, &self.occlusion_texture]
            .into_iter()
            .flatten()
    }
}

/// Сведения о модели без загрузки на GPU: размеры, части, материалы.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshInfo {
    pub vertices: usize,
    pub triangles: usize,
    pub bounds: (Vec3, Vec3),
    /// Части модели: группы `o`/`g` в OBJ, треугольные примитивы glTF.
    pub submeshes: usize,
    pub materials: Vec<MeshMaterial>,
    /// Внешние файлы, от которых зависит модель (буферы, `.mtl`, текстуры) — пути
    /// относительно файла модели. От их изменения меняется и картинка предпросмотра.
    pub files: Vec<String>,
}

/// То, что можно узнать о модели, не разбирая её вершины: части, материалы, внешние файлы.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshExtras {
    pub submeshes: usize,
    pub materials: Vec<MeshMaterial>,
    pub files: Vec<String>,
}

/// Части, материалы и внешние файлы модели. Дёшево: OBJ просматривается построчно,
/// у glTF читается только JSON. Аргументы как у [`load_mesh_bytes`]; нечитаемая
/// модель даёт пустой результат (ошибку покажет разбор вершин).
pub fn mesh_extras(name: &str, bytes: &[u8], resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> MeshExtras {
    let is_obj = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("obj"));
    let (submeshes, materials, files) = if is_obj {
        obj_extras(&String::from_utf8_lossy(bytes), resolve)
    } else if engine_model_formats::Format::detect(name, bytes).is_some() {
        (1, Vec::new(), engine_model_formats::dependencies(name, bytes))
    } else {
        gltf_extras(bytes)
    };
    MeshExtras {
        submeshes: submeshes.max(1),
        materials,
        files,
    }
}

/// Разобрать модель и собрать [`MeshInfo`]. Аргументы как у [`load_mesh_bytes`].
pub fn inspect_mesh(
    name: &str,
    bytes: &[u8],
    resolve: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<MeshInfo, AssetError> {
    let data = load_mesh_bytes(name, bytes, resolve)?;
    let extras = mesh_extras(name, bytes, resolve);
    Ok(MeshInfo {
        vertices: data.vertex_count(),
        triangles: data.triangle_count(),
        bounds: data.bounds(),
        submeshes: extras.submeshes,
        materials: extras.materials,
        files: extras.files,
    })
}

fn obj_extras(text: &str, resolve: &dyn Fn(&str) -> Option<Vec<u8>>) -> (usize, Vec<MeshMaterial>, Vec<String>) {
    let (mut groups, mut files) = (0, Vec::new());
    let mut used: Vec<String> = Vec::new();
    let mut libraries = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match (parts.next(), parts.next()) {
            (Some("o" | "g"), Some(_)) => groups += 1,
            (Some("usemtl"), Some(name)) if !used.iter().any(|u| u == name) => used.push(name.to_string()),
            (Some("mtllib"), Some(file)) => libraries.push(file.to_string()),
            _ => {}
        }
    }
    let mut all: Vec<MeshMaterial> = Vec::new();
    for library in libraries {
        files.push(library.clone());
        let Some(bytes) = resolve(&library) else { continue };
        for line in String::from_utf8_lossy(&bytes).lines() {
            let mut parts = line.split_whitespace();
            let keyword = parts.next();
            if keyword == Some("newmtl") {
                all.push(MeshMaterial::named(parts.next().unwrap_or(""), [0.8, 0.8, 0.8, 1.0]));
                continue;
            }
            let Some(m) = all.last_mut() else { continue };
            let rest: Vec<&str> = parts.collect();
            let numbers: Vec<f32> = rest.iter().filter_map(|p| p.parse().ok()).collect();
            // Параметры вида `-s 1 1 1` стоят перед именем файла: берём последнее слово.
            let file = rest.last().map(|f| f.to_string());
            match (keyword, numbers.as_slice()) {
                (Some("Kd"), [r, g, b, ..]) => m.color = [*r, *g, *b, m.color[3]],
                (Some("Ke"), [r, g, b, ..]) => m.emissive = [*r, *g, *b],
                (Some("d"), [d, ..]) => m.color[3] = d.clamp(0.0, 1.0),
                (Some("Tr"), [t, ..]) => m.color[3] = (1.0 - t).clamp(0.0, 1.0),
                // Блеск Фонга 0..1000 → шероховатость.
                (Some("Ns"), [ns, ..]) => m.roughness = (1.0 - (ns / 1000.0).clamp(0.0, 1.0).sqrt()).clamp(0.05, 1.0),
                (Some("Pr"), [r, ..]) => m.roughness = *r,
                (Some("Pm"), [metal, ..]) => m.metallic = *metal,
                (Some("map_Kd"), _) => m.texture = file,
                (Some("map_Ke"), _) => m.emissive_texture = file,
                (Some("map_Bump" | "map_bump" | "bump" | "norm"), _) => m.normal_texture = file,
                _ => {}
            }
        }
    }
    for m in &mut all {
        if m.color[3] < 1.0 {
            m.alpha_mode = AlphaMode::Blend;
        }
    }
    all.retain(|m| used.is_empty() || used.contains(&m.name));
    files.extend(all.iter().flat_map(|m| m.texture_paths().cloned().collect::<Vec<_>>()));
    (groups, all, files)
}

#[cfg(not(feature = "gltf"))]
fn gltf_extras(_bytes: &[u8]) -> (usize, Vec<MeshMaterial>, Vec<String>) {
    (0, Vec::new(), Vec::new())
}

#[cfg(feature = "gltf")]
fn gltf_extras(bytes: &[u8]) -> (usize, Vec<MeshMaterial>, Vec<String>) {
    let Ok(gltf) = gltf::Gltf::from_slice(bytes) else {
        return (0, Vec::new(), Vec::new());
    };
    let document = gltf.document;
    let submeshes = document
        .meshes()
        .flat_map(|mesh| mesh.primitives())
        .filter(|primitive| primitive.mode() == gltf::mesh::Mode::Triangles)
        .count();
    let external = |uri: &str| (!uri.starts_with("data:")).then(|| uri.to_string());
    let mut files: Vec<String> = document
        .buffers()
        .filter_map(|buffer| match buffer.source() {
            gltf::buffer::Source::Uri(uri) => external(uri),
            gltf::buffer::Source::Bin => None,
        })
        .collect();
    let materials: Vec<MeshMaterial> = document
        .materials()
        .map(|material| {
            let pbr = material.pbr_metallic_roughness();
            let image = |texture: gltf::Texture| match texture.source().source() {
                gltf::image::Source::Uri { uri, .. } => external(uri),
                gltf::image::Source::View { .. } => None,
            };
            let name = material.name().map(str::to_string).unwrap_or_else(|| match material.index() {
                Some(index) => format!("material {index}"),
                None => "material".to_string(),
            });
            MeshMaterial {
                name,
                color: pbr.base_color_factor(),
                texture: pbr.base_color_texture().and_then(|info| image(info.texture())),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                metallic_roughness_texture: pbr.metallic_roughness_texture().and_then(|info| image(info.texture())),
                normal_texture: material.normal_texture().and_then(|info| image(info.texture())),
                normal_scale: material.normal_texture().map_or(1.0, |info| info.scale()),
                emissive: material.emissive_factor(),
                emissive_texture: material.emissive_texture().and_then(|info| image(info.texture())),
                occlusion_texture: material.occlusion_texture().and_then(|info| image(info.texture())),
                alpha_mode: match material.alpha_mode() {
                    gltf::material::AlphaMode::Opaque => AlphaMode::Opaque,
                    gltf::material::AlphaMode::Mask => AlphaMode::Cutout,
                    gltf::material::AlphaMode::Blend => AlphaMode::Blend,
                },
                alpha_cutoff: material.alpha_cutoff().unwrap_or(0.5),
                double_sided: material.double_sided(),
            }
        })
        .collect();
    files.extend(materials.iter().flat_map(|m| m.texture_paths().cloned().collect::<Vec<_>>()));
    files.dedup();
    (submeshes, materials, files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_reads_obj_materials_and_groups() {
        let obj = "mtllib m.mtl\no a\nv 0 0 0\nv 1 0 0\nv 0 1 0\nusemtl red\nf 1 2 3\no b\nf 1 2 3\n";
        let mtl = "newmtl red\nKd 1 0 0\nmap_Kd -s 1 1 1 red.png\nnewmtl unused\nKd 0 1 0\n";
        let resolve = |name: &str| (name == "m.mtl").then(|| mtl.as_bytes().to_vec());
        let info = inspect_mesh("x.obj", obj.as_bytes(), &resolve).unwrap();
        assert_eq!((info.vertices, info.triangles, info.submeshes), (3, 2, 2));
        assert_eq!(info.materials.len(), 1, "only materials used by the model are listed");
        assert_eq!(info.materials[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(info.materials[0].texture.as_deref(), Some("red.png"));
        assert_eq!(info.files, ["m.mtl", "red.png"]);
    }

    #[test]
    fn obj_usemtl_makes_submeshes_with_material_numbers() {
        let obj = "mtllib m.mtl\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\n\
                   usemtl red\nf 1 2 3\nf 2 4 3\nusemtl glass\nf 1 3 4\nusemtl red\nf 1 2 4\n";
        let mtl = "newmtl glass\nKd 0.5 0.7 1\nd 0.3\nNs 900\nnewmtl red\nKd 1 0 0\nKe 2 0 0\nmap_Bump -bm 1 n.png\n";
        let resolve = |name: &str| (name == "m.mtl").then(|| mtl.as_bytes().to_vec());
        let mesh = load_mesh_bytes("x.obj", obj.as_bytes(), &resolve).unwrap();
        let parts: Vec<(Range<u32>, Option<usize>, &str)> =
            mesh.submeshes.iter().map(|s| (s.indices.clone(), s.material, s.name.as_str())).collect();
        assert_eq!(parts, [(0..6, Some(1), "red"), (6..9, Some(0), "glass"), (9..12, Some(1), "red")]);
        let extras = mesh_extras("x.obj", obj.as_bytes(), &resolve);
        assert_eq!(extras.materials[0].alpha_mode, AlphaMode::Blend);
        assert!(extras.materials[0].roughness < 0.1);
        assert_eq!(extras.materials[1].emissive, [2.0, 0.0, 0.0]);
        assert_eq!(extras.materials[1].normal_texture.as_deref(), Some("n.png"));
        assert!(extras.files.contains(&"n.png".to_string()));
    }

    #[test]
    fn generated_tangents_follow_u_and_point_up_the_image() {
        // Квадрат в плоскости XZ: U растёт вдоль +X, V (вниз по картинке) — вдоль +Z.
        let mut mesh = parse_obj(
            "v 0 0 0\nv 1 0 0\nv 1 0 1\nv 0 0 1\nvt 0 1\nvt 1 1\nvt 1 0\nvt 0 0\nvn 0 1 0\n\
             f 1/1/1 4/4/1 3/3/1\nf 1/1/1 3/3/1 2/2/1\n",
        )
        .unwrap();
        mesh.compute_tangents();
        assert_eq!(mesh.tangents.len(), 4);
        for t in &mesh.tangents {
            assert!((Vec3::new(t[0], t[1], t[2]) - Vec3::X).length() < 1e-4, "{t:?}");
            // cross(N, T) * w — «вверх» по картинке, то есть против роста V (−Z).
            let up = Vec3::Y.cross(Vec3::X) * t[3];
            assert!((up - Vec3::NEG_Z).length() < 1e-4, "{t:?}");
        }
    }

    #[test]
    fn obj_quad_is_triangulated_and_gets_normals() {
        let mesh = parse_obj(
            "# a unit quad in the XZ plane\n\
             v 0 0 0\nv 1 0 0\nv 1 0 -1\nv 0 0 -1\n\
             vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\n\
             f 1/1 2/2 3/3 4/4\n",
        )
        .unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.indices, [0, 1, 2, 0, 2, 3]);
        // Нормалей в файле нет: вычислены, смотрят вверх (обход против часовой стрелки).
        assert!(mesh.normals.iter().all(|n| (Vec3::from_array(*n) - Vec3::Y).length() < 1e-5));
        // Ось V перевёрнута: (0, 0) в OBJ — левый нижний угол.
        assert_eq!(mesh.uvs[0], [0.0, 1.0]);
        assert_eq!(mesh.bounds(), (Vec3::new(0.0, 0.0, -1.0), Vec3::new(1.0, 0.0, 0.0)));
    }

    #[test]
    fn obj_supports_negative_indices_and_shares_vertices() {
        let mesh = parse_obj(
            "v 0 0 0\nv 1 0 0\nv 0 1 0\nv 1 1 0\nvn 0 0 1\n\
             f -4//1 -3//1 -2//1\nf 2//1 4//1 3//1\n",
        )
        .unwrap();
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.vertex_count(), 4, "vertices shared by both faces are not duplicated");
        assert!(mesh.normals.iter().all(|n| *n == [0.0, 0.0, 1.0]));
    }

    #[test]
    fn obj_errors_name_the_line() {
        let error = parse_obj("v 0 0 0\nv 1 0 zero\n").unwrap_err();
        assert_eq!(error, "line 2: `zero` is not a number");
        let error = parse_obj("v 0 0 0\nf 1 2 3\n").unwrap_err();
        assert_eq!(error, "line 2: vertex index 2 is out of range (the file has 1)");
    }
}
