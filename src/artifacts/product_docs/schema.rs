use crate::artifacts::planning_store::PlanningRoot;
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

pub fn load_documents<R: PlanningRoot + ?Sized>(
    repo: &R,
) -> anyhow::Result<Option<Vec<ProductDocument>>> {
    let store = repo.planning_store();
    if store.published_repository_root.is_some() {
        return load_published_documents(repo);
    }
    #[cfg(not(test))]
    return Ok(None);
    #[cfg(test)]
    {
        let layout = repo.planning_layout();
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
            let content = String::from_utf8(repo.read_planning_path(&path)?)?;
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
}

pub fn load_modules<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Option<Vec<String>>> {
    Ok(load_documents(repo)?.map(|documents| {
        documents
            .into_iter()
            .map(|document| document.content)
            .collect()
    }))
}

pub fn render_product<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Option<String>> {
    let Some(documents) = load_documents(repo)? else {
        return Ok(None);
    };
    let index = read_product_index(repo)?;
    Ok(Some(render_documents(&index, &documents)))
}

/// Render only documents read from the supplied immutable code-repository
/// revision. Private drafts and indexes never participate in this result.
pub fn render_published_product(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
) -> anyhow::Result<Option<String>> {
    let (index, documents) = load_published_product(reader, pin)?;
    Ok((!documents.is_empty()).then(|| render_documents(&index, &documents)))
}

pub fn render_published_product_with_updates(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
    updates: &[(String, String)],
) -> anyhow::Result<Option<String>> {
    let (index, mut documents) = load_published_product(reader, pin)?;
    if documents.is_empty() {
        return Ok(None);
    }
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
    Ok(Some(render_documents(&index, &documents)))
}

pub fn render_product_with_updates<R: PlanningRoot + ?Sized>(
    repo: &R,
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
    let index = read_product_index(repo)?;
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

fn load_published_documents<R: PlanningRoot + ?Sized>(
    repo: &R,
) -> anyhow::Result<Option<Vec<ProductDocument>>> {
    let store = repo.planning_store();
    let Some(code_root) = store.published_repository_root.as_deref() else {
        return Ok(None);
    };
    let reader =
        crate::core::published_specs::PublishedSpecReader::for_primary_repository(code_root)?;
    let pin = match &store.published_spec_pin {
        Some(pin) => pin.clone(),
        None => reader.pin()?,
    };
    let (_, documents) = load_published_product(&reader, &pin)?;
    Ok((!documents.is_empty()).then_some(documents))
}

pub(crate) fn load_published_product(
    reader: &crate::core::published_specs::PublishedSpecReader,
    pin: &crate::core::published_specs::PublishedSpecPin,
) -> anyhow::Result<(String, Vec<ProductDocument>)> {
    let published = reader.read_documentation(pin)?;
    let index = published
        .iter()
        .find(|document| document.path == "docs/koolade/product/index.md")
        .map(|document| document.content.clone())
        .unwrap_or_else(|| "# Product\n".into());
    Ok((index, published_product_documents(published)?))
}

pub(crate) fn published_product_documents(
    published: Vec<crate::core::published_specs::PublishedDocument>,
) -> anyhow::Result<Vec<ProductDocument>> {
    let mut documents = Vec::new();
    for document in published.into_iter().filter(|document| {
        document.path.starts_with("docs/koolade/product/")
            && document.path != "docs/koolade/product/index.md"
    }) {
        let relative = document
            .path
            .strip_prefix("docs/koolade/product/")
            .expect("selected product document");
        let path = Path::new(relative);
        let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
            anyhow::bail!("published product document has an invalid file name");
        };
        anyhow::ensure!(
            super::valid_id(id),
            "published product document ID is invalid"
        );
        validate_module(&document.content)?;
        let title = module_title(&document.content)
            .map(str::to_owned)
            .unwrap_or_else(|| id.replace('-', " "));
        let core_concept = CoreConcept::all()
            .into_iter()
            .find(|concept| concept.id() == id);
        let mut module = ProductModule::new(id, &title, core_concept);
        module.uid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, document.path.as_bytes())
            .hyphenated()
            .to_string();
        module.path = relative.to_owned();
        documents.push(ProductDocument {
            module,
            content: document.content,
        });
    }
    documents.sort_by(|left, right| left.module.path.cmp(&right.module.path));
    Ok(documents)
}

fn read_product_index<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<String> {
    let store = repo.planning_store();
    if let Some(code_root) = store.published_repository_root.as_deref() {
        let reader =
            crate::core::published_specs::PublishedSpecReader::for_primary_repository(code_root)?;
        let pin = match &store.published_spec_pin {
            Some(pin) => pin.clone(),
            None => reader.pin()?,
        };
        return Ok(reader
            .read_optional(&pin, "docs/koolade/product/index.md")?
            .map(|document| document.content)
            .unwrap_or_else(|| "# Product\n".into()));
    }
    #[cfg(not(test))]
    anyhow::bail!("published product index requires the primary code repository");
    #[cfg(test)]
    {
        match repo.read_planning_path(&repo.planning_layout().product_index()) {
            Ok(bytes) => Ok(String::from_utf8(bytes)?),
            Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                Ok("# Product\n".into())
            }
            Err(error) => Err(error.into()),
        }
    }
}
