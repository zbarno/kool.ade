use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use std::path::Path;

use super::{CoreConcept, ProductModule};

#[derive(Debug, Clone)]
pub struct ProductDocument {
    pub module: ProductModule,
    pub content: String,
}

pub fn validate_module(text: &str) -> anyhow::Result<()> {
    anyhow::ensure!(!text.trim().is_empty(), "Product module must not be blank");
    let headings = headings(text);
    let top = headings
        .first()
        .ok_or_else(|| anyhow::anyhow!("Product module requires a title heading"))?;
    anyhow::ensure!(
        top.0 == HeadingLevel::H1 || top.0 == HeadingLevel::H2,
        "Product module must begin with an H1 or H2 title"
    );
    anyhow::ensure!(
        headings
            .iter()
            .filter(|(level, _)| *level == HeadingLevel::H1)
            .count()
            <= 1,
        "Product module may contain at most one H1"
    );
    anyhow::ensure!(
        !module_title(text).unwrap_or_default().trim().is_empty(),
        "Product module title must not be blank"
    );
    Ok(())
}

pub fn module_title(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let line = line.trim();
        let title = line
            .strip_prefix("# ")
            .or_else(|| line.strip_prefix("## "))?;
        Some(title.trim())
    })
}

fn headings(text: &str) -> Vec<(HeadingLevel, String)> {
    let mut output = Vec::new();
    let mut active = None;
    for event in Parser::new(text) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => active = Some((level, String::new())),
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, title)) = &mut active {
                    title.push_str(&value);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = active.take() {
                    output.push(heading);
                }
            }
            _ => {}
        }
    }
    output
}

pub fn split_sections(text: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut starts = Vec::new();
    let mut active = None;
    for (event, range) in Parser::new(text).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H2,
                ..
            }) => {
                active = Some((range.start, String::new()));
            }
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, title)) = &mut active {
                    title.push_str(&value);
                }
            }
            Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
                if let Some(heading) = active.take() {
                    starts.push(heading);
                }
            }
            _ => {}
        }
    }
    anyhow::ensure!(
        !starts.is_empty(),
        "Specification requires at least one H2 module"
    );
    Ok(starts
        .iter()
        .enumerate()
        .map(|(index, (start, title))| {
            let end = starts.get(index + 1).map_or(text.len(), |next| next.0);
            (
                title.trim().to_owned(),
                text[*start..end].trim_end().to_owned() + "\n",
            )
        })
        .collect())
}

/// Compatibility parser for the old thirteen-section monolithic document.
pub fn split_legacy(text: &str) -> anyhow::Result<[String; 13]> {
    let sections = split_sections(text)?;
    anyhow::ensure!(
        sections.len() == super::LEGACY_MODULES.len(),
        "Legacy specification must have exactly thirteen top-level sections"
    );
    for (index, (title, _)) in sections.iter().enumerate() {
        anyhow::ensure!(
            title.starts_with(&format!("{}. ", index + 1)),
            "Legacy section {} is missing or out of order",
            index + 1
        );
    }
    Ok(sections
        .into_iter()
        .map(|(_, content)| content)
        .collect::<Vec<_>>()
        .try_into()
        .expect("thirteen legacy sections"))
}

pub fn load_documents(repo: &Path) -> anyhow::Result<Option<Vec<ProductDocument>>> {
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    let root = layout.product_root();
    if !super::feature_index::real_dir(&root)? {
        return Ok(None);
    }
    anyhow::ensure!(
        super::feature_index::regular(&layout.product_index())?,
        "Product index is missing"
    );
    let Some(manifest) = super::load_manifest(repo)? else {
        return Ok(None);
    };
    manifest.validate(&root, true)?;
    let mut documents = Vec::with_capacity(manifest.modules.len());
    for module in manifest.modules {
        let path = super::safe_module_path(&root, &module.path).unwrap();
        let content = std::fs::read_to_string(&path)?;
        validate_module(&content)?;
        anyhow::ensure!(
            module_title(&content).is_some_and(|title| title == module.title),
            "Product module {} title differs from its manifest",
            module.id
        );
        documents.push(ProductDocument { module, content });
    }
    Ok(Some(documents))
}

pub fn load_modules(repo: &Path) -> anyhow::Result<Option<Vec<String>>> {
    Ok(load_documents(repo)?.map(|documents| {
        documents
            .into_iter()
            .map(|document| document.content)
            .collect()
    }))
}

pub fn render_product(repo: &Path) -> anyhow::Result<Option<String>> {
    let Some(documents) = load_documents(repo)? else {
        return Ok(None);
    };
    let index = std::fs::read_to_string(
        crate::artifacts::layout::ArtifactLayout::new(repo).product_index(),
    )?;
    Ok(Some(render_documents(&index, &documents)))
}

pub fn render_product_with_updates(
    repo: &Path,
    updates: &[(String, String)],
) -> anyhow::Result<Option<String>> {
    let Some(mut documents) = load_documents(repo)? else {
        return Ok(None);
    };
    for (id, content) in updates {
        let Some(module_id) = id.strip_prefix("product:") else {
            continue;
        };
        if module_id == "index" {
            continue;
        }
        validate_module(content)?;
        if let Some(document) = documents
            .iter_mut()
            .find(|document| document.module.id == module_id)
        {
            document.module.title = module_title(content).unwrap().to_owned();
            document.content = content.clone();
        } else if super::valid_id(module_id) {
            documents.push(ProductDocument {
                module: ProductModule::new(module_id, module_title(content).unwrap(), None),
                content: content.clone(),
            });
        }
    }
    let index = std::fs::read_to_string(
        crate::artifacts::layout::ArtifactLayout::new(repo).product_index(),
    )?;
    Ok(Some(render_documents(&index, &documents)))
}

fn render_documents(index: &str, documents: &[ProductDocument]) -> String {
    let title = index
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or("Product")
        .trim_end_matches(" — Living Technical Specification");
    let mut output = format!("# {title} — Living Technical Specification\n\n");
    for document in documents {
        let section = document
            .module
            .core_concept
            .map(CoreConcept::title)
            .unwrap_or(&document.module.title);
        output.push_str(&format!(
            "## {section}\n\n{}\n",
            without_title(&document.content)
        ));
    }
    output
}

fn without_title(markdown: &str) -> &str {
    let Some((first, rest)) = markdown.split_once('\n') else {
        return "";
    };
    if first.trim_start().starts_with('#') {
        rest.trim_start()
    } else {
        markdown
    }
}
