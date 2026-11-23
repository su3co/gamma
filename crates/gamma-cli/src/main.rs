//! `gamma`: the hub, the MCP server, the simulation worker and the project
//! commands, in one program.

mod client;
mod mcp;
mod worker;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use client::{Client, enc};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "gamma",
    version,
    about = "gamma: design, simulate and test photonic integrated circuits (hub, MCP server, worker and project commands)"
)]
struct Cli {
    /// Project folder (default: the current folder or a parent with gamma.toml)
    #[arg(long, short = 'C', global = true)]
    project: Option<PathBuf>,
    /// Print JSON instead of text
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the hub (one per machine and user)
    Hub {
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Folder of the built interface to serve to browsers
        #[arg(long)]
        ui: Option<PathBuf>,
        /// Exit after this many idle minutes (0: never)
        #[arg(long, default_value_t = 0)]
        idle_exit_min: u64,
        /// Print the browser address and keep running even if a hub is already up
        #[arg(long)]
        print_url: bool,
    },
    /// MCP server for agents on standard input/output
    Mcp,
    /// Run one simulation and stream progress as JSON lines (used by the hub, locally or on a remote host)
    Worker {
        #[command(subcommand)]
        cmd: WorkerCmd,
    },
    /// Create a project in a folder (empty, or around a GDS file from any design tool)
    Init {
        dir: Option<PathBuf>,
        #[arg(long)]
        name: Option<String>,
        /// Start from this GDS file (copied into layout/)
        #[arg(long)]
        gds: Option<PathBuf>,
        /// Layer stack (stack.toml); default: generic 220 nm SOI
        #[arg(long)]
        stack: Option<PathBuf>,
        /// Ports (JSON or CSV: name,x,y,width,orientation,layer,datatype); default: from the GDS metadata if any
        #[arg(long)]
        ports: Option<PathBuf>,
    },
    /// Export a run's S-parameters (Touchstone .sNp or Lumerical INTERCONNECT .dat)
    Export {
        sim: String,
        #[arg(long)]
        run: Option<String>,
        /// touchstone or lumerical
        #[arg(long, default_value = "touchstone")]
        format: String,
        /// Also copy the file here
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Open a project in the hub and print its id and address
    Open { dir: Option<PathBuf> },
    /// Project overview
    Info,
    /// Regenerate files from their sources (all, layout, sim:<name>)
    Build {
        #[arg(default_value = "all")]
        target: String,
    },
    /// What a simulation describes, the grid and run-time estimates
    Setup {
        sim: String,
        #[arg(long)]
        preview: bool,
    },
    /// Run a simulation
    Run {
        sim: String,
        /// cpu, cuda:0, cuda:0,1
        #[arg(long)]
        device: Option<String>,
        /// Remote host name
        #[arg(long)]
        host: Option<String>,
        #[arg(long)]
        preview: bool,
        /// Return at once instead of following the job
        #[arg(long)]
        detach: bool,
    },
    /// List jobs, or follow/cancel one
    Jobs {
        id: Option<String>,
        #[arg(long)]
        cancel: bool,
    },
    /// Results of a run (latest by default)
    Results {
        sim: String,
        #[arg(long)]
        run: Option<String>,
        #[arg(long, default_value = "overview")]
        what: String,
        #[arg(long)]
        monitor: Option<String>,
    },
    /// Design-rule and consistency checks
    Check,
    /// Render a viewer to PNG: gamma view layout -o out.png [key=value ...]
    View {
        view: String,
        #[arg(short, long)]
        out: PathBuf,
        params: Vec<String>,
    },
    /// Create a checkpoint, or list them
    Checkpoint {
        message: Option<String>,
        #[arg(long)]
        list: bool,
    },
    /// Compare two checkpoints (or one with the current state)
    Diff { a: u32, b: Option<u32> },
    /// Remote simulation hosts
    Remote {
        #[command(subcommand)]
        cmd: RemoteCmd,
    },
    /// Connect an agent (Claude Code, Codex) to this project
    Agent {
        #[command(subcommand)]
        cmd: AgentCmd,
    },
    /// Stop the hub
    Stop,
}

#[derive(Subcommand)]
enum WorkerCmd {
    /// Run a lightsim spec
    Run {
        spec: PathBuf,
        #[arg(long, default_value = "cpu")]
        device: String,
        #[arg(long)]
        out: Option<PathBuf>,
        /// auto, none, or COMPONENT,AXIS,COORDINATE
        #[arg(long, default_value = "auto")]
        slice: String,
        /// Seconds between progress messages
        #[arg(long, default_value_t = 1.0)]
        every: f64,
    },
    /// Version and devices of this machine
    Info,
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// Add (or replace) a host
    Add {
        name: String,
        /// user@host or an SSH config alias
        ssh: String,
        /// Command prefix for a job queue; with {host} it runs locally and reaches the host itself
        #[arg(long)]
        launcher: Option<String>,
        /// Path of gamma on the host
        #[arg(long)]
        program: Option<String>,
        /// The launcher exposes only the GPUs it grants (they are cuda:0.. inside the job)
        #[arg(long)]
        renumber_gpus: bool,
    },
    List,
    Remove {
        name: String,
    },
    /// Check the connection, the installed gamma and its GPUs
    Test {
        name: String,
    },
    /// Copy a gamma build for the host's platform to the host
    Install {
        name: String,
        #[arg(long)]
        binary: PathBuf,
    },
}

#[derive(Subcommand)]
enum AgentCmd {
    /// Write the MCP configuration an agent CLI reads in this project (.mcp.json for Claude Code; prints the Codex command)
    Setup {
        #[arg(default_value = "claude")]
        provider: String,
    },
    /// Send a message to the in-app agent session
    Send { text: String },
}

fn project_dir(cli: &Cli) -> Result<PathBuf> {
    let d = cli.project.clone().unwrap_or(std::env::current_dir()?);
    Ok(gamma_core::Project::discover(&d)?.root)
}

fn print(cli: &Cli, v: &Value, human: impl FnOnce(&Value) -> String) {
    if cli.json {
        println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
    } else {
        println!("{}", human(v));
    }
}

fn fmt_s(s: f64) -> String {
    if s < 90.0 {
        format!("{s:.0} s")
    } else if s < 5400.0 {
        format!("{:.0} min", s / 60.0)
    } else {
        format!("{:.1} h", s / 3600.0)
    }
}

fn find_ui_dir(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(d) = explicit.or_else(|| std::env::var_os("GAMMA_UI_DIR").map(PathBuf::from)) {
        return Some(d);
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    // a source checkout first (the build also copies the interface next to
    // the binary, where it may be older), then the installed locations
    let cands = [
        dir.join("../../app/ui/dist"),
        dir.join("ui"),
        dir.join("../share/gamma/ui"),
        dir.join("../Resources/ui"),
        dir.join("../lib/gamma/ui"),
    ];
    cands.into_iter().find(|c| c.join("index.html").is_file())
}

fn run_hub(port: u16, ui: Option<PathBuf>, idle_exit_min: u64, print_url: bool) -> Result<()> {
    if let Some(i) = gamma_hub::read_hub_info()
        && gamma_hub::discovery::alive(&i)
    {
        println!("a hub is already running at {} (pid {})", i.url(), i.pid);
        if print_url {
            println!("{}/?token={}", i.url(), i.token);
        }
        return Ok(());
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .init();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let token: String = (0..32).map(|_| format!("{:x}", rand::random::<u8>() & 0xf)).collect();
        let worker = std::env::current_exe()?;
        let ui_dir = find_ui_dir(ui);
        let hub = gamma_hub::Hub::new(token.clone(), worker, ui_dir.clone());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.context("binding the hub's port")?;
        let addr = listener.local_addr()?;
        let info = gamma_hub::HubInfo { port: addr.port(), pid: std::process::id(), token: token.clone(), version: gamma_core::VERSION.into() };
        gamma_hub::discovery::write_hub_info(&info)?;
        eprintln!("gamma hub {} on {}{}", gamma_core::VERSION, info.url(), if ui_dir.is_some() { "" } else { " (no interface folder: API only)" });
        if print_url {
            println!("{}/?token={}", info.url(), token);
        }
        let app = gamma_hub::api::router(hub.clone());
        let pid = std::process::id();
        let h2 = hub.clone();
        let shutdown = async move {
            let idle = async {
                if idle_exit_min == 0 {
                    std::future::pending::<()>().await;
                }
                let mut idle_since = std::time::Instant::now();
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    let busy = h2.jobs.active() > 0 || !h2.agents.sessions.lock().unwrap().is_empty();
                    if busy {
                        idle_since = std::time::Instant::now();
                    } else if idle_since.elapsed() > Duration::from_secs(idle_exit_min * 60) {
                        break;
                    }
                }
            };
            #[cfg(unix)]
            {
                use tokio::signal::unix::{SignalKind, signal};
                let mut term = signal(SignalKind::terminate()).unwrap();
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {}, _ = idle => {} }
            }
            #[cfg(not(unix))]
            {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = idle => {} }
            }
        };
        axum::serve(listener, app).with_graceful_shutdown(shutdown).await?;
        gamma_hub::discovery::remove_hub_info(pid);
        eprintln!("gamma hub stopped");
        Ok::<(), anyhow::Error>(())
    })
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = real_main(&cli) {
        eprintln!("gamma: {e:#}");
        std::process::exit(1);
    }
}

fn real_main(cli: &Cli) -> Result<()> {
    match &cli.cmd {
        Cmd::Hub {
            port,
            ui,
            idle_exit_min,
            print_url,
        } => run_hub(*port, ui.clone(), *idle_exit_min, *print_url),
        Cmd::Mcp => mcp::serve(cli.project.clone()),
        Cmd::Worker { cmd } => match cmd {
            WorkerCmd::Run {
                spec,
                device,
                out,
                slice,
                every,
            } => worker::run_worker(spec, device, out.as_deref(), slice, *every),
            WorkerCmd::Info => {
                println!("{}", worker::hello().line());
                Ok(())
            }
        },
        Cmd::Init {
            dir,
            name,
            gds,
            stack,
            ports,
        } => {
            let d = dir.clone().unwrap_or(std::env::current_dir()?);
            match gds {
                Some(g) => {
                    std::fs::create_dir_all(&d)?;
                    gamma_hub::projects::init_from_gds(
                        &d,
                        name.as_deref(),
                        g,
                        stack.as_deref(),
                        ports.as_deref(),
                    )?;
                }
                None => gamma_hub::projects::init_project(&d, name.as_deref())?,
            }
            println!("created a gamma project in {}", d.display());
            Ok(())
        }
        Cmd::Stop => {
            match gamma_hub::read_hub_info() {
                Some(i) if gamma_hub::discovery::alive(&i) => {
                    #[cfg(unix)]
                    {
                        std::process::Command::new("kill")
                            .arg(i.pid.to_string())
                            .status()?;
                    }
                    println!("stopped the hub (pid {})", i.pid);
                }
                _ => println!("no hub is running"),
            }
            Ok(())
        }
        _ => project_command(cli),
    }
}

fn project_command(cli: &Cli) -> Result<()> {
    let c = Client::connect("cli")?;
    let root = project_dir(cli)?;
    let id = c.open(&root)?;
    let p = |rest: &str| format!("/api/p/{id}{rest}");
    match &cli.cmd {
        Cmd::Open { .. } => {
            let v = c.get(&p(""))?;
            print(cli, &v, |v| {
                format!(
                    "{} ({})\nhub: {}\nbrowser: {}/?token={}",
                    v["name"].as_str().unwrap_or(""),
                    id,
                    c.info.url(),
                    c.info.url(),
                    c.info.token
                )
            });
        }
        Cmd::Info => {
            let v = c.get(&p(""))?;
            let l = c.get(&p("/layout"))?;
            let s = c.get(&p("/sims"))?;
            let all = json!({"project": v, "layout": l["layout"], "sims": s});
            print(cli, &all, |_| {
                let mut o = format!(
                    "{}  {}\n",
                    v["name"].as_str().unwrap_or(""),
                    v["root"].as_str().unwrap_or("")
                );
                if let Some(lay) = l["layout"].as_object() {
                    o += &format!(
                        "layout: cell {} with {} polygons on {} layers, {} ports\n",
                        lay["top"].as_str().unwrap_or(""),
                        lay["polygons"],
                        lay["layers"].as_array().map(|a| a.len()).unwrap_or(0),
                        lay["ports"].as_array().map(|a| a.len()).unwrap_or(0)
                    );
                } else {
                    o += "layout: not built\n";
                }
                for x in s.as_array().cloned().unwrap_or_default() {
                    o += &format!(
                        "simulation {}: {}{}\n",
                        x["name"].as_str().unwrap_or(""),
                        x["spec"].as_str().unwrap_or(""),
                        if x["built"] == true {
                            ""
                        } else {
                            " (not built)"
                        }
                    );
                }
                o.trim_end().to_string()
            });
        }
        Cmd::Build { target } => {
            let v = c.post(&p("/build"), json!({"target": target}))?;
            print(cli, &v, |v| {
                v.as_array()
                    .map(|a| {
                        a.iter()
                            .map(|x| {
                                format!(
                                    "{}: {} in {:.1} s",
                                    x["target"].as_str().unwrap_or(""),
                                    if x["ok"] == true { "ok" } else { "failed" },
                                    x["seconds"].as_f64().unwrap_or(0.0)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            });
        }
        Cmd::Setup { sim, preview } => {
            let v = c.get(&p(&format!(
                "/sims/{}/setup?preview={}",
                enc(sim),
                *preview as u8
            )))?;
            print(cli, &v, |v| {
                let g = &v["grid"];
                let mut o = format!(
                    "{}: {} x {} x {} = {:.2} M cells, {} steps of {:.3} fs, about {:.2} GB\n",
                    v["name"].as_str().unwrap_or(""),
                    g["n"][0],
                    g["n"][1],
                    g["n"][2],
                    g["cells"].as_f64().unwrap_or(0.0) / 1e6,
                    g["steps"],
                    g["dt"].as_f64().unwrap_or(0.0) * 1e15,
                    g["memory_bytes"].as_f64().unwrap_or(0.0) / 1e9
                );
                for e in v["estimates"].as_array().cloned().unwrap_or_default() {
                    o += &format!(
                        "  {:<10} {:<36} ~{}\n",
                        e["device"].as_str().unwrap_or(""),
                        e["name"].as_str().unwrap_or(""),
                        fmt_s(e["seconds"].as_f64().unwrap_or(0.0))
                    );
                }
                for ob in v["objects"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|o| o["kind"] != "structure")
                {
                    o += &format!(
                        "  {} {} ({})\n",
                        ob["kind"].as_str().unwrap_or(""),
                        ob["name"].as_str().unwrap_or(""),
                        ob["type"].as_str().unwrap_or("")
                    );
                }
                o.trim_end().to_string()
            });
        }
        Cmd::Run {
            sim,
            device,
            host,
            preview,
            detach,
        } => {
            let job = c.post(
                &p(&format!("/sims/{}/run", enc(sim))),
                json!({"device": device, "host": host, "preview": preview}),
            )?;
            let jid = job["id"].as_str().unwrap_or("").to_string();
            if *detach {
                print(cli, &job, |j| {
                    format!(
                        "queued job {} ({} on {})",
                        j["id"].as_str().unwrap_or(""),
                        j["sim"].as_str().unwrap_or(""),
                        j["host"].as_str().unwrap_or("this machine")
                    )
                });
                return Ok(());
            }
            eprintln!(
                "job {jid}: {sim} on {}{}",
                job["host"]
                    .as_str()
                    .map(|h| format!("{h} "))
                    .unwrap_or_default(),
                job["device"].as_str().unwrap_or("")
            );
            let mut last = String::new();
            let j = c.wait_job(&jid, None, |j| {
                let pr = &j["progress"];
                let line = if pr.is_null() {
                    format!("  {}", j["status"].as_str().unwrap_or(""))
                } else {
                    format!(
                        "  step {}/{}  decay {}  {} Mcells/s",
                        pr["step"],
                        pr["steps"],
                        pr["decay"]
                            .as_f64()
                            .map(|d| format!("{d:.1e}"))
                            .unwrap_or("-".into()),
                        pr["mcells_per_s"]
                            .as_f64()
                            .map(|r| format!("{r:.0}"))
                            .unwrap_or("-".into())
                    )
                };
                if line != last {
                    eprintln!("{line}");
                    last = line;
                }
            })?;
            print(cli, &j, |j| match j["status"].as_str() {
                Some("done") => format!(
                    "done: results/{}/{}",
                    j["sim"].as_str().unwrap_or(""),
                    j["run_id"].as_str().unwrap_or("")
                ),
                s => format!("{}: {}", s.unwrap_or(""), j["error"].as_str().unwrap_or("")),
            });
            if j["status"] != "done" {
                bail!("the run did not finish");
            }
        }
        Cmd::Jobs { id, cancel } => match id {
            Some(i) if *cancel => print(
                cli,
                &c.post(&format!("/api/jobs/{}/cancel", enc(i)), json!({}))?,
                |j| {
                    format!(
                        "{}: {}",
                        j["id"].as_str().unwrap_or(""),
                        j["status"].as_str().unwrap_or("")
                    )
                },
            ),
            Some(i) => print(cli, &c.get(&format!("/api/jobs/{}", enc(i)))?, |j| {
                serde_json::to_string_pretty(j).unwrap_or_default()
            }),
            None => print(cli, &c.get("/api/jobs")?, |v| {
                v.as_array()
                    .map(|a| {
                        a.iter()
                            .map(|j| {
                                format!(
                                    "{}  {:<10} {:<16} {} {}",
                                    j["id"].as_str().unwrap_or(""),
                                    j["status"].as_str().unwrap_or(""),
                                    j["sim"].as_str().unwrap_or(""),
                                    j["host"].as_str().unwrap_or("local"),
                                    j["device"].as_str().unwrap_or("")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            }),
        },
        Cmd::Results {
            sim,
            run,
            what,
            monitor,
        } => {
            let run = match run {
                Some(r) => r.clone(),
                None => {
                    let runs = c.get(&p(&format!("/runs?sim={}", enc(sim))))?;
                    runs.as_array()
                        .and_then(|a| a.iter().find(|r| r["has_results"] == true))
                        .and_then(|r| r["id"].as_str().map(|s| s.to_string()))
                        .context("no finished run")?
                }
            };
            let mut url = p(&format!("/runs/{}/{}/{}", enc(sim), enc(&run), enc(what)));
            if let Some(m) = monitor {
                url += &format!("?monitor={}", enc(m));
            }
            let v = c.get(&url)?;
            print(cli, &v, |v| {
                serde_json::to_string_pretty(v).unwrap_or_default()
            });
        }
        Cmd::Export { sim, run, format, out } => {
            let run = match run {
                Some(r) => r.clone(),
                None => {
                    let runs = c.get(&p(&format!("/runs?sim={}", enc(sim))))?;
                    runs.as_array().and_then(|a| a.iter().find(|r| r["has_results"] == true)).and_then(|r| r["id"].as_str().map(|s| s.to_string())).context("no finished run")?
                }
            };
            let v = c.post(&p(&format!("/runs/{}/{}/export", enc(sim), enc(&run))), json!({"format": format}))?;
            if let Some(o) = out {
                std::fs::write(o, v["text"].as_str().unwrap_or(""))?;
            }
            println!("wrote {}{}", v["path"].as_str().unwrap_or(""), out.as_ref().map(|o| format!(" and {}", o.display())).unwrap_or_default());
        }
        Cmd::Check => {
            let v = c.get(&p("/check"))?;
            print(cli, &v, |v| {
                let f = v["findings"].as_array().cloned().unwrap_or_default();
                if f.is_empty() {
                    return "no findings".into();
                }
                f.iter()
                    .map(|x| {
                        format!(
                            "{:<8} {:<22} {}",
                            x["severity"].as_str().unwrap_or(""),
                            x["rule"].as_str().unwrap_or(""),
                            x["message"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            });
        }
        Cmd::View { view, out, params } => {
            let mut q = format!("view={}", enc(view));
            for kv in params {
                let (k, v) = kv
                    .split_once('=')
                    .with_context(|| format!("parameter '{kv}' should be key=value"))?;
                q += &format!("&{}={}", enc(k), enc(v));
            }
            let png = c.get_bytes(&p(&format!("/render?{q}")))?;
            std::fs::write(out, &png)?;
            println!("wrote {} ({} bytes)", out.display(), png.len());
        }
        Cmd::Checkpoint { message, list } => {
            if *list {
                let v = c.get(&p("/checkpoints"))?;
                print(cli, &v, |v| {
                    v["checkpoints"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| {
                                    format!(
                                        "{:>3}  {}  {:<20} {}",
                                        x["n"],
                                        &x["commit"].as_str().unwrap_or("")
                                            [..8.min(x["commit"].as_str().unwrap_or("").len())],
                                        x["actor"].as_str().unwrap_or(""),
                                        x["message"].as_str().unwrap_or("")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default()
                });
            } else {
                let v = c.post(
                    &p("/checkpoints"),
                    json!({"message": message.clone().unwrap_or_else(|| "Checkpoint".into())}),
                )?;
                print(cli, &v, |v| {
                    if v.is_null() {
                        "nothing changed since the last checkpoint".into()
                    } else {
                        format!("checkpoint {}", v["n"])
                    }
                });
            }
        }
        Cmd::Diff { a, b } => {
            let mut url = p(&format!("/checkpoints/diff?a={a}"));
            if let Some(b) = b {
                url += &format!("&b={b}");
            }
            let v = c.get(&url)?;
            print(cli, &v, |v| {
                serde_json::to_string_pretty(v).unwrap_or_default()
            });
        }
        Cmd::Remote { cmd } => match cmd {
            RemoteCmd::Add {
                name,
                ssh,
                launcher,
                program,
                renumber_gpus,
            } => print(
                cli,
                &c.post(
                    "/api/remotes",
                    json!({"name": name, "ssh": ssh, "launcher": launcher, "program": program, "renumber_gpus": renumber_gpus}),
                )?,
                |v| format!("added remote {}", v["name"].as_str().unwrap_or("")),
            ),
            RemoteCmd::List => print(cli, &c.get("/api/remotes")?, |v| {
                v.as_array()
                    .map(|a| {
                        a.iter()
                            .map(|r| {
                                format!(
                                    "{:<16} {}",
                                    r["name"].as_str().unwrap_or(""),
                                    r["ssh"].as_str().unwrap_or("")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default()
            }),
            RemoteCmd::Remove { name } => print(
                cli,
                &c.delete(&format!("/api/remotes/{}", enc(name)))?,
                |_| format!("removed {name}"),
            ),
            RemoteCmd::Test { name } => print(
                cli,
                &c.post(&format!("/api/remotes/{}/test", enc(name)), json!({}))?,
                |v| serde_json::to_string_pretty(v).unwrap_or_default(),
            ),
            RemoteCmd::Install { name, binary } => print(
                cli,
                &c.post(
                    &format!("/api/remotes/{}/install", enc(name)),
                    json!({"binary": binary.canonicalize()?}),
                )?,
                |v| serde_json::to_string_pretty(v).unwrap_or_default(),
            ),
        },
        Cmd::Agent { cmd } => match cmd {
            AgentCmd::Setup { provider } => agent_setup(&root, provider)?,
            AgentCmd::Send { text } => {
                c.post(&p("/agent/send"), json!({"text": text}))?;
                println!("sent");
            }
        },
        _ => unreachable!(),
    }
    Ok(())
}

fn agent_setup(root: &Path, provider: &str) -> Result<()> {
    let exe = std::env::current_exe()?;
    gamma_hub::agent::install_skills(root)?;
    match provider {
        "claude" => {
            let path = root.join(".mcp.json");
            let mut v: Value = std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(json!({"mcpServers": {}}));
            v["mcpServers"]["gamma"] = json!({"command": exe, "args": ["mcp"]});
            gamma_core::fsutil::atomic_write(&path, serde_json::to_string_pretty(&v)?.as_bytes())?;
            println!(
                "wrote {} — start `claude` in {} and the gamma tools are available (approve the server when asked)",
                path.display(),
                root.display()
            );
        }
        "codex" => {
            println!(
                "add to ~/.codex/config.toml:\n\n[mcp_servers.gamma]\ncommand = \"{}\"\nargs = [\"mcp\"]\n\nthen run `codex` in {}",
                exe.display(),
                root.display()
            );
        }
        other => bail!("unknown agent '{other}': claude or codex"),
    }
    Ok(())
}
