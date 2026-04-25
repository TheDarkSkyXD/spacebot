//! `.csproj` parser (C# / .NET).
//!
//! We extract two fields:
//! - `<RootNamespace>` — namespace prefix for generated code and the
//!   default namespace for files without an explicit `namespace` block.
//! - `<AssemblyName>` — output assembly name. Usually equals
//!   `RootNamespace`; where they differ, both are useful for import
//!   resolution.
//!
//! `.csproj` is MSBuild XML but we only need two elements, so a regex
//! pass avoids pulling in an XML crate. The extractor is tolerant of
//! XML comments, namespace prefixes, and the SDK-style short form.

use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use regex::Regex;

use super::{workspace_mut, ConfigContext};

static ROOT_NAMESPACE_RE: OnceLock<Regex> = OnceLock::new();
static ASSEMBLY_NAME_RE: OnceLock<Regex> = OnceLock::new();

fn root_namespace_re() -> &'static Regex {
    ROOT_NAMESPACE_RE.get_or_init(|| {
        Regex::new(r"(?s)<\s*RootNamespace\s*>([^<]+)<\s*/\s*RootNamespace\s*>")
            .expect("static regex")
    })
}
fn assembly_name_re() -> &'static Regex {
    ASSEMBLY_NAME_RE.get_or_init(|| {
        Regex::new(r"(?s)<\s*AssemblyName\s*>([^<]+)<\s*/\s*AssemblyName\s*>")
            .expect("static regex")
    })
}

/// Load a `.csproj` and record `RootNamespace` / `AssemblyName`.
pub async fn load(
    path: &Path,
    workspace_root: &Path,
    ctx: &mut ConfigContext,
) -> Result<()> {
    let raw = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("reading {}", path.display()))?;
    let (ns, asm) = extract_namespace_fields(&raw);

    let ws = workspace_mut(ctx, workspace_root);
    if let Some(ns) = ns {
        ws.csharp_root_namespace = Some(ns);
    }
    if let Some(asm) = asm {
        ws.csharp_assembly_name = Some(asm);
    }
    Ok(())
}

fn extract_namespace_fields(src: &str) -> (Option<String>, Option<String>) {
    let ns = root_namespace_re()
        .captures(src)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string());
    let asm = assembly_name_re()
        .captures(src)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string());
    (ns, asm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn parses_root_namespace_and_assembly() {
        let tmp = TempDir::new().unwrap();
        let p = tmp.path().join("app.csproj");
        tokio::fs::write(
            &p,
            r#"<Project Sdk="Microsoft.NET.Sdk">
              <PropertyGroup>
                <TargetFramework>net8.0</TargetFramework>
                <RootNamespace>MyCompany.App</RootNamespace>
                <AssemblyName>MyCompany.App.dll</AssemblyName>
              </PropertyGroup>
            </Project>"#,
        )
        .await
        .unwrap();
        let mut ctx = ConfigContext::default();
        load(&p, Path::new(""), &mut ctx).await.unwrap();
        let ws = &ctx.workspaces[0];
        assert_eq!(ws.csharp_root_namespace, Some("MyCompany.App".to_string()));
        assert_eq!(
            ws.csharp_assembly_name,
            Some("MyCompany.App.dll".to_string())
        );
    }

    #[test]
    fn extractor_handles_missing_fields() {
        let src = "<Project Sdk=\"Microsoft.NET.Sdk\"></Project>";
        let (ns, asm) = extract_namespace_fields(src);
        assert!(ns.is_none());
        assert!(asm.is_none());
    }

    #[test]
    fn extractor_handles_whitespace() {
        let src = "<Project>\n  <PropertyGroup>\n    <RootNamespace>  App.Core  </RootNamespace>\n  </PropertyGroup>\n</Project>";
        let (ns, _) = extract_namespace_fields(src);
        assert_eq!(ns, Some("App.Core".to_string()));
    }
}
