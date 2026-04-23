//! Maven (`pom.xml`) and Gradle (`build.gradle[.kts]`) parser.
//!
//! For JVM imports we need (group, artifact) coordinates so downstream
//! resolution can distinguish e.g. `com.example:foo-api` from
//! `com.example:foo-impl` when both appear in a monorepo. Classpath
//! resolution across modules is a Phase 2+ concern; here we just record
//! what's in the manifest.
//!
//! Both Maven POMs and Gradle scripts have enough internal variance
//! that a regex-based scan is the pragmatic choice — the alternative
//! (XML parser + Groovy/Kotlin AST parsing) is vastly more complexity
//! for the same output shape.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;

use super::{workspace_mut, ConfigContext, JvmCoords};

static POM_GROUP_RE: OnceLock<Regex> = OnceLock::new();
static POM_ARTIFACT_RE: OnceLock<Regex> = OnceLock::new();
static GRADLE_GROUP_RE: OnceLock<Regex> = OnceLock::new();
static GRADLE_ARTIFACT_RE: OnceLock<Regex> = OnceLock::new();

fn pom_group_re() -> &'static Regex {
    POM_GROUP_RE.get_or_init(|| {
        Regex::new(r"(?s)<\s*groupId\s*>([^<]+)<\s*/\s*groupId\s*>")
            .expect("static regex")
    })
}
fn pom_artifact_re() -> &'static Regex {
    POM_ARTIFACT_RE.get_or_init(|| {
        Regex::new(r"(?s)<\s*artifactId\s*>([^<]+)<\s*/\s*artifactId\s*>")
            .expect("static regex")
    })
}
// Gradle: `group = "com.example"` or `group 'com.example'`.
fn gradle_group_re() -> &'static Regex {
    GRADLE_GROUP_RE.get_or_init(|| {
        Regex::new(r#"(?m)^\s*group\s*[=]?\s*['"]([^'"]+)['"]"#).expect("static regex")
    })
}
fn gradle_artifact_re() -> &'static Regex {
    // Multiple spellings: `rootProject.name = "x"`, `archivesBaseName = "x"`,
    // or a `project(':name')` include — pick the first that matches.
    GRADLE_ARTIFACT_RE.get_or_init(|| {
        Regex::new(
            r#"(?m)^\s*(?:rootProject\.name|archivesBaseName)\s*[=]?\s*['"]([^'"]+)['"]"#,
        )
        .expect("static regex")
    })
}

/// Load a JVM manifest (Maven or Gradle) and record coordinates.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let coords = if name == "pom.xml" {
        extract_pom_coords(&raw)
    } else {
        extract_gradle_coords(&raw)
    };
    if coords.group_id.is_some() || coords.artifact_id.is_some() {
        workspace_mut(ctx, workspace_root).jvm_coords = Some(coords);
    }
    Ok(())
}

fn extract_pom_coords(src: &str) -> JvmCoords {
    JvmCoords {
        group_id: pom_group_re()
            .captures(src)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string()),
        artifact_id: pom_artifact_re()
            .captures(src)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string()),
    }
}

fn extract_gradle_coords(src: &str) -> JvmCoords {
    JvmCoords {
        group_id: gradle_group_re()
            .captures(src)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string()),
        artifact_id: gradle_artifact_re()
            .captures(src)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_pom_xml() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("pom.xml");
        tokio::fs::write(
            &p,
            r#"<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>my-app</artifactId>
  <version>1.0.0</version>
</project>"#,
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let coords = ctx.workspaces[0].jvm_coords.as_ref().unwrap();
        assert_eq!(coords.group_id, Some("com.example".to_string()));
        assert_eq!(coords.artifact_id, Some("my-app".to_string()));
    }

    #[tokio::test]
    async fn parses_build_gradle_kts() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("build.gradle.kts");
        tokio::fs::write(
            &p,
            r#"plugins { `kotlin-dsl` }

group = "com.example.service"
version = "0.1.0"
rootProject.name = "order-service"
"#,
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let coords = ctx.workspaces[0].jvm_coords.as_ref().unwrap();
        assert_eq!(coords.group_id, Some("com.example.service".to_string()));
        assert_eq!(coords.artifact_id, Some("order-service".to_string()));
    }
}
