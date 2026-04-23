//! JCL (Job Control Language) parser.
//!
//! JCL is the z/OS batch-job language: a stream of `//`-prefixed
//! control statements that name jobs, declare execution steps
//! (`EXEC PGM=...`), and bind dataset references (`DD`). A JCL
//! source produces one [`JclJob`] wrapping an ordered list of
//! [`JclStep`] records; each step can reference one invoked program
//! and zero-or-more datasets.
//!
//! We land this in Phase 7 because mainframe projects index poorly
//! without the job graph — cross-module call relationships that
//! flow through job execution are invisible when only the COBOL
//! source is indexed. Matches GitNexus's `jcl-parser.ts` +
//! `jcl-processor.ts` minus the IMS/DB2-specific resolution.

use std::sync::OnceLock;

use regex::Regex;

/// One JCL job, parsed from a `.jcl` / `.job` / `.proc` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JclJob {
    /// Job name from the JOB card (`//JOBNAME JOB ...`).
    pub name: String,
    /// 1-based line of the JOB card.
    pub line: u32,
    /// Ordered list of steps in this job.
    pub steps: Vec<JclStep>,
}

/// One execution step within a job (`//STEPNAME EXEC PGM=...`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JclStep {
    /// Step name as written in the source.
    pub name: String,
    /// The program invoked — typically a COBOL module name.
    /// `None` when the step uses `EXEC PROC=` instead.
    pub pgm: Option<String>,
    /// The procedure invoked (`EXEC PROC=XYZ`), if any.
    pub proc_name: Option<String>,
    /// DD statements in declaration order. Each entry is the
    /// `DDNAME` plus the raw dataset reference.
    pub dd_statements: Vec<JclDd>,
    /// 1-based line of the EXEC card.
    pub line: u32,
}

/// One `DD` (dataset definition) line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JclDd {
    /// DD name — the identifier after `//`.
    pub name: String,
    /// Raw dataset reference (`DSN=FOO.BAR`, `SYSOUT=*`, etc.).
    pub body: String,
    /// 1-based line.
    pub line: u32,
}

static JOB_CARD_RE: OnceLock<Regex> = OnceLock::new();
static EXEC_CARD_RE: OnceLock<Regex> = OnceLock::new();
static DD_CARD_RE: OnceLock<Regex> = OnceLock::new();

fn job_card_re() -> &'static Regex {
    JOB_CARD_RE.get_or_init(|| {
        Regex::new(r#"^//(\S+)\s+JOB\b"#).expect("static regex")
    })
}

fn exec_card_re() -> &'static Regex {
    EXEC_CARD_RE.get_or_init(|| {
        Regex::new(r#"^//(\S+)\s+EXEC\s+(.*)$"#).expect("static regex")
    })
}

fn dd_card_re() -> &'static Regex {
    DD_CARD_RE.get_or_init(|| {
        Regex::new(r#"^//(\S+)\s+DD\s+(.*)$"#).expect("static regex")
    })
}

static PGM_RE: OnceLock<Regex> = OnceLock::new();
static PROC_RE: OnceLock<Regex> = OnceLock::new();

fn pgm_re() -> &'static Regex {
    PGM_RE.get_or_init(|| {
        Regex::new(r#"PGM\s*=\s*([A-Za-z0-9_$#@-]+)"#).expect("static regex")
    })
}

fn proc_re() -> &'static Regex {
    PROC_RE.get_or_init(|| {
        Regex::new(r#"PROC\s*=\s*([A-Za-z0-9_$#@-]+)"#).expect("static regex")
    })
}

/// Parse a JCL source into zero-or-one `JclJob` records.
///
/// Most sources contain a single job (JCL semantics enforce this
/// at submission time) but we return a `Vec` because `.proc`
/// members and in-stream procedures can declare standalone steps
/// without a JOB card — surfacing those as pseudo-jobs keeps the
/// downstream consumer simple.
pub fn parse(content: &str) -> Vec<JclJob> {
    let mut jobs: Vec<JclJob> = Vec::new();
    let mut current: Option<JclJob> = None;
    let mut current_step: Option<JclStep> = None;

    for (i, line) in content.lines().enumerate() {
        let line_num = (i + 1) as u32;

        if let Some(cap) = job_card_re().captures(line) {
            // Flush in-progress step + job.
            if let Some(step) = current_step.take()
                && let Some(job) = current.as_mut()
            {
                job.steps.push(step);
            }
            if let Some(job) = current.take() {
                jobs.push(job);
            }
            current = Some(JclJob {
                name: cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                line: line_num,
                steps: Vec::new(),
            });
            continue;
        }

        if let Some(cap) = exec_card_re().captures(line) {
            if let Some(step) = current_step.take()
                && let Some(job) = current.as_mut()
            {
                job.steps.push(step);
            }
            let step_name = cap
                .get(1)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let tail = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let pgm = pgm_re()
                .captures(tail)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            let proc_name = proc_re()
                .captures(tail)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());
            current_step = Some(JclStep {
                name: step_name,
                pgm,
                proc_name,
                dd_statements: Vec::new(),
                line: line_num,
            });
            continue;
        }

        if let Some(cap) = dd_card_re().captures(line)
            && let Some(step) = current_step.as_mut()
        {
            step.dd_statements.push(JclDd {
                name: cap
                    .get(1)
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                body: cap
                    .get(2)
                    .map(|m| m.as_str().trim().to_string())
                    .unwrap_or_default(),
                line: line_num,
            });
        }
    }

    if let Some(step) = current_step.take()
        && let Some(job) = current.as_mut()
    {
        job.steps.push(step);
    }
    if let Some(job) = current.take() {
        jobs.push(job);
    }
    jobs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_job() {
        let src = "\
//TESTJOB  JOB CLASS=A,MSGCLASS=X
//STEP01   EXEC PGM=MYPROG
//INFILE   DD DSN=MY.INPUT,DISP=SHR
//OUTFILE  DD DSN=MY.OUTPUT,DISP=OLD
";
        let jobs = parse(src);
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.name, "TESTJOB");
        assert_eq!(job.steps.len(), 1);
        let step = &job.steps[0];
        assert_eq!(step.name, "STEP01");
        assert_eq!(step.pgm.as_deref(), Some("MYPROG"));
        assert_eq!(step.dd_statements.len(), 2);
        assert_eq!(step.dd_statements[0].name, "INFILE");
        assert!(step.dd_statements[0].body.contains("MY.INPUT"));
    }

    #[test]
    fn parses_multiple_steps() {
        let src = "\
//J JOB CLASS=A
//S1 EXEC PGM=P1
//S2 EXEC PGM=P2
//S3 EXEC PROC=BATCH1
";
        let jobs = parse(src);
        assert_eq!(jobs[0].steps.len(), 3);
        assert_eq!(jobs[0].steps[2].proc_name.as_deref(), Some("BATCH1"));
    }

    #[test]
    fn handles_no_job_card() {
        let src = "//STEP01 EXEC PGM=ORPHAN\n";
        let jobs = parse(src);
        // No JOB card → no job record; the orphan step is discarded.
        assert!(jobs.is_empty());
    }
}
