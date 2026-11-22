//! The hub's HTTP and WebSocket API (loopback only, token required).

use crate::events::Event;
use crate::jobs::RunRequest;
use crate::projects::{ProjectState, file_tree};
use crate::state::Hub;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use gamma_core::refs::ObjRef;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error": self.1}))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for ApiError {
    fn from(e: E) -> Self {
        let e: anyhow::Error = e.into();
        let msg = format!("{e:#}");
        let code = if msg.starts_with("no ") || msg.contains("not found") || msg.contains("does not exist") { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
        ApiError(code, msg)
    }
}

type R<T> = Result<T, ApiError>;
type Q = Query<HashMap<String, String>>;

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> R<T> {
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r.map_err(ApiError::from),
        Err(e) => Err(ApiError(StatusCode::INTERNAL_SERVER_ERROR, format!("internal error: {e}"))),
    }
}

fn bin(data: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "application/octet-stream")], data).into_response()
}

fn png(data: Vec<u8>) -> Response {
    ([(header::CONTENT_TYPE, "image/png"), (header::CACHE_CONTROL, "no-store")], data).into_response()
}

async fn auth(State(hub): State<Hub>, req: Request, next: Next) -> Response {
    // DNS rebinding: only loopback host names
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let hostname = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    if !matches!(hostname, "127.0.0.1" | "localhost" | "[::1]" | "") {
        return (StatusCode::FORBIDDEN, "forbidden host").into_response();
    }
    let path = req.uri().path();
    if !path.starts_with("/api/") {
        return next.run(req).await;
    }
    let ok_header = req.headers().get(header::AUTHORIZATION).and_then(|h| h.to_str().ok()).map(|s| s == format!("Bearer {}", hub.token)).unwrap_or(false);
    let ok_query = req.uri().query().map(|q| q.split('&').any(|kv| kv == format!("token={}", hub.token))).unwrap_or(false);
    if !(ok_header || ok_query) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing or wrong token"}))).into_response();
    }
    next.run(req).await
}

pub fn router(hub: Hub) -> Router {
    let api = Router::new()
        .route("/api/hub", get(hub_info))
        .route("/api/open", post(open))
        .route("/api/init", post(init))
        .route("/api/jobs", get(jobs))
        .route("/api/jobs/{id}", get(job))
        .route("/api/jobs/{id}/cancel", post(job_cancel))
        .route("/api/jobs/{id}/slice", get(job_slice))
        .route("/api/remotes", get(remotes).post(remote_add))
        .route("/api/remotes/{name}", axum::routing::delete(remote_remove))
        .route("/api/remotes/{name}/test", post(remote_test))
        .route("/api/remotes/{name}/install", post(remote_install))
        .route("/api/agents/providers", get(|| async { Json(crate::agent::providers()) }))
        .route("/api/config", get(config_get).put(config_put))
        .route("/api/p/{id}", get(project_info))
        .route("/api/p/{id}/files", get(files))
        .route("/api/p/{id}/file", get(file_get).put(file_put))
        .route("/api/p/{id}/build", post(build))
        .route("/api/p/{id}/autobuild", post(autobuild))
        .route("/api/p/{id}/layout", get(layout))
        .route("/api/p/{id}/layout/mesh", get(layout_mesh))
        .route("/api/p/{id}/layout/outline", get(layout_outline))
        .route("/api/p/{id}/layout/mesh3d", get(layout_mesh3d))
        .route("/api/p/{id}/layout/pick", get(layout_pick))
        .route("/api/p/{id}/layout/gap", get(layout_gap))
        .route("/api/p/{id}/layout/query", get(layout_query))
        .route("/api/p/{id}/stack", get(stack_get).put(stack_put))
        .route("/api/p/{id}/sims", get(sims))
        .route("/api/p/{id}/sims/{name}/setup", get(sim_setup))
        .route("/api/p/{id}/sims/{name}/slice", get(sim_slice))
        .route("/api/p/{id}/sims/{name}/run", post(sim_run))
        .route("/api/p/{id}/runs", get(runs))
        .route("/api/p/{id}/runs/{sim}/{run}/{what}", get(run_view))
        .route("/api/p/{id}/check", get(check))
        .route("/api/p/{id}/render", get(render))
        .route("/api/p/{id}/selection", get(selection_get).post(selection_set))
        .route("/api/p/{id}/checkpoints", get(checkpoints).post(checkpoint_create))
        .route("/api/p/{id}/checkpoints/diff", get(checkpoint_diff))
        .route("/api/p/{id}/comments", get(comments).post(comment_add))
        .route("/api/p/{id}/events", get(events))
        .route("/api/p/{id}/ws", get(ws))
        .route("/api/p/{id}/agent", get(agent_status))
        .route("/api/p/{id}/agent/start", post(agent_start))
        .route("/api/p/{id}/agent/send", post(agent_send))
        .route("/api/p/{id}/agent/stop", post(agent_stop))
        .route("/api/p/{id}/circuits", get(circuits))
        .route("/api/p/{id}/circuits/{name}", get(circuit));
    let mut app = api.with_state(hub.clone());
    if let Some(dir) = &hub.ui_dir {
        app = app.fallback_service(tower_http::services::ServeDir::new(dir).append_index_html_on_directories(true));
    } else {
        app = app.fallback(|| async { (StatusCode::NOT_FOUND, "gamma hub: no interface folder configured (API under /api)") });
    }
    app.layer(middleware::from_fn_with_state(hub.clone(), auth))
        .layer(tower_http::cors::CorsLayer::new().allow_origin(tower_http::cors::Any).allow_headers(tower_http::cors::Any).allow_methods(tower_http::cors::Any))
}

fn actor(h: &HeaderMap) -> String {
    h.get("x-gamma-actor").and_then(|v| v.to_str().ok()).unwrap_or("person").to_string()
}

fn proj(hub: &Hub, id: &str) -> R<Arc<ProjectState>> {
    Ok(hub.project(id)?)
}

async fn hub_info(State(hub): State<Hub>) -> Json<Value> {
    Json(hub.info())
}

#[derive(Deserialize)]
struct OpenReq {
    path: String,
}

async fn open(State(hub): State<Hub>, Json(r): Json<OpenReq>) -> R<Json<Value>> {
    let h = hub.clone();
    let ps = blocking(move || {
        let p = std::path::Path::new(&r.path);
        let root = gamma_core::Project::discover(p)?.root;
        Ok(root)
    })
    .await?;
    let ps = hub.open(&ps)?;
    let _ = h;
    Ok(Json(ps.info()))
}

#[derive(Deserialize)]
struct InitReq {
    path: String,
    name: Option<String>,
}

async fn init(State(hub): State<Hub>, Json(r): Json<InitReq>) -> R<Json<Value>> {
    let root = std::path::PathBuf::from(&r.path);
    let root2 = root.clone();
    blocking(move || crate::projects::init_project(&root2, r.name.as_deref())).await?;
    let ps = hub.open(&root)?;
    Ok(Json(ps.info()))
}

async fn jobs(State(hub): State<Hub>) -> Json<Value> {
    Json(json!(hub.jobs.list()))
}

async fn job(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    Ok(Json(json!(hub.jobs.get(&id).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, "no such job".into()))?)))
}

async fn job_cancel(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    Ok(Json(json!(hub.jobs.cancel(&id)?)))
}

async fn job_slice(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    Ok(Json(hub.jobs.last_slice(&id).unwrap_or(Value::Null)))
}

async fn remotes(State(hub): State<Hub>) -> Json<Value> {
    Json(json!(hub.remotes(None)))
}

async fn remote_add(State(hub): State<Hub>, Json(r): Json<gamma_core::project::RemoteCfg>) -> R<Json<Value>> {
    if r.name.is_empty() || r.ssh.is_empty() {
        return Err(bad("a remote needs a name and an ssh target"));
    }
    let mut c = hub.config.lock().unwrap();
    c.remotes.retain(|x| x.name != r.name);
    c.remotes.push(r.clone());
    c.save()?;
    Ok(Json(json!(r)))
}

async fn remote_remove(State(hub): State<Hub>, Path(name): Path<String>) -> R<Json<Value>> {
    let mut c = hub.config.lock().unwrap();
    c.remotes.retain(|x| x.name != name);
    c.save()?;
    Ok(Json(json!({"removed": name})))
}

async fn remote_test(State(hub): State<Hub>, Path(name): Path<String>) -> R<Json<Value>> {
    let r = hub.remote(&name, None)?;
    Ok(Json(crate::remote::test(&r).await?))
}

#[derive(Deserialize)]
struct InstallReq {
    binary: String,
}

async fn remote_install(State(hub): State<Hub>, Path(name): Path<String>, Json(b): Json<InstallReq>) -> R<Json<Value>> {
    let r = hub.remote(&name, None)?;
    Ok(Json(crate::remote::install(&r, std::path::Path::new(&b.binary)).await?))
}

async fn config_get(State(hub): State<Hub>) -> Json<Value> {
    Json(json!(*hub.config.lock().unwrap()))
}

async fn config_put(State(hub): State<Hub>, Json(c): Json<crate::config::UserConfig>) -> R<Json<Value>> {
    c.save()?;
    *hub.config.lock().unwrap() = c.clone();
    Ok(Json(json!(c)))
}

async fn project_info(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let mut v = ps.info();
    v["agent"] = hub.agents.status(&ps);
    v["checkpoints"] = json!(crate::git::list(&ps.root()).unwrap_or_default().len());
    Ok(Json(v))
}

async fn files(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let root = ps.root();
    Ok(Json(json!(blocking(move || Ok(file_tree(&root))).await?)))
}

async fn file_get(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Response> {
    let ps = proj(&hub, &id)?;
    let p = ps.safe_path(q.get("path").ok_or_else(|| bad("path required"))?)?;
    let data = tokio::fs::read(&p).await.map_err(|e| ApiError(StatusCode::NOT_FOUND, format!("{}: {e}", p.display())))?;
    if data.len() > 8 << 20 {
        return Err(bad("file larger than 8 MB"));
    }
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], data).into_response())
}

async fn file_put(State(hub): State<Hub>, Path(id): Path<String>, headers: HeaderMap, Query(q): Q, body: Bytes) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let rel = q.get("path").ok_or_else(|| bad("path required"))?.clone();
    let p = ps.safe_path(&rel)?;
    gamma_core::fsutil::atomic_write(&p, &body)?;
    ps.log.push(&actor(&headers), "file.written", json!({"path": rel, "bytes": body.len()}));
    Ok(Json(json!({"written": rel})))
}

#[derive(Deserialize)]
struct BuildReq {
    #[serde(default = "all")]
    target: String,
}
fn all() -> String {
    "all".into()
}

async fn build(State(hub): State<Hub>, Path(id): Path<String>, headers: HeaderMap, Json(b): Json<BuildReq>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let out = ps.build(&b.target, &actor(&headers)).await?;
    Ok(Json(json!(out)))
}

#[derive(Deserialize)]
struct OnReq {
    on: bool,
}

async fn autobuild(State(hub): State<Hub>, Path(id): Path<String>, Json(b): Json<OnReq>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    ps.autobuild.store(b.on, std::sync::atomic::Ordering::Relaxed);
    Ok(Json(json!({"autobuild": b.on})))
}

fn layer_key(q: &HashMap<String, String>) -> R<(u16, u16)> {
    let s = q.get("layer").ok_or_else(|| bad("layer required (e.g. 1/0)"))?;
    let (a, b) = s.split_once('/').unwrap_or((s, "0"));
    Ok((a.parse().map_err(|_| bad("bad layer"))?, b.parse().map_err(|_| bad("bad layer"))?))
}

async fn layout(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    blocking(move || {
        let l = ps.layout()?;
        let stack = ps.stack();
        Ok(Json(match l {
            Some(l) => json!({"layout": l.summary(stack.as_deref()), "stack": stack.as_deref()}),
            None => json!({"layout": null, "stack": stack.as_deref()}),
        }))
    })
    .await
}

async fn layout_mesh(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Response> {
    let ps = proj(&hub, &id)?;
    let key = layer_key(&q)?;
    let data = blocking(move || ps.layout()?.and_then(|l| l.layer_mesh_bytes(key)).ok_or_else(|| anyhow::anyhow!("no layer {}/{}", key.0, key.1))).await?;
    Ok(bin(data))
}

async fn layout_outline(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Response> {
    let ps = proj(&hub, &id)?;
    let key = layer_key(&q)?;
    let data = blocking(move || ps.layout()?.and_then(|l| l.layer_outline_bytes(key)).ok_or_else(|| anyhow::anyhow!("no layer {}/{}", key.0, key.1))).await?;
    Ok(bin(data))
}

async fn layout_mesh3d(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Response> {
    let ps = proj(&hub, &id)?;
    let key = layer_key(&q)?;
    let data = blocking(move || {
        let l = ps.layout()?.ok_or_else(|| anyhow::anyhow!("no layout"))?;
        let stack = ps.stack().ok_or_else(|| anyhow::anyhow!("no layer stack"))?;
        let sl = stack.layer_for(key.0, key.1).ok_or_else(|| anyhow::anyhow!("layer {}/{} is not in the stack", key.0, key.1))?;
        let ld = l.layer(key).ok_or_else(|| anyhow::anyhow!("no layer"))?;
        let mut m = gamma_core::geom::Mesh::default();
        for p in &ld.polygons {
            m.extrude(p, sl.zmin, sl.zmax());
        }
        let mut out = Vec::with_capacity(8 + m.positions.len() * 8 + m.indices.len() * 4);
        out.extend(((m.positions.len() / 3) as u32).to_le_bytes());
        out.extend((m.indices.len() as u32).to_le_bytes());
        for v in &m.positions {
            out.extend(v.to_le_bytes());
        }
        for v in &m.normals {
            out.extend(v.to_le_bytes());
        }
        for v in &m.indices {
            out.extend(v.to_le_bytes());
        }
        Ok(out)
    })
    .await?;
    Ok(bin(data))
}

async fn layout_pick(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let x: f64 = q.get("x").and_then(|v| v.parse().ok()).ok_or_else(|| bad("x required"))?;
    let y: f64 = q.get("y").and_then(|v| v.parse().ok()).ok_or_else(|| bad("y required"))?;
    let tol: f64 = q.get("tol").and_then(|v| v.parse().ok()).unwrap_or(0.05);
    blocking(move || {
        let l = ps.layout()?.ok_or_else(|| anyhow::anyhow!("no layout"))?;
        Ok(Json(json!(l.pick([x, y], tol, ps.stack().as_deref()))))
    })
    .await
}

async fn layout_gap(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let a = q.get("a").and_then(|s| ObjRef::parse(s)).ok_or_else(|| bad("a=poly:L/D#i required"))?;
    let b = q.get("b").and_then(|s| ObjRef::parse(s)).ok_or_else(|| bad("b=poly:L/D#i required"))?;
    blocking(move || {
        let l = ps.layout()?.ok_or_else(|| anyhow::anyhow!("no layout"))?;
        let k = |r: &ObjRef| match r {
            ObjRef::Poly { layer, datatype, index } => Ok(((*layer, *datatype), *index)),
            _ => Err(anyhow::anyhow!("gap needs two polygon references")),
        };
        let (d, p, q) = l.gap(k(&a)?, k(&b)?).ok_or_else(|| anyhow::anyhow!("no such polygon"))?;
        Ok(Json(json!({"distance_um": d, "from": p, "to": q})))
    })
    .await
}

async fn layout_query(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let key = layer_key(&q)?;
    let f = |k: &str| q.get(k).and_then(|v| v.parse::<f64>().ok());
    let b = gamma_core::geom::BBox { min: [f("x0").unwrap_or(f64::MIN), f("y0").unwrap_or(f64::MIN)], max: [f("x1").unwrap_or(f64::MAX), f("y1").unwrap_or(f64::MAX)] };
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(200);
    blocking(move || {
        let l = ps.layout()?.ok_or_else(|| anyhow::anyhow!("no layout"))?;
        let ld = l.layer(key).ok_or_else(|| anyhow::anyhow!("no layer {}/{}", key.0, key.1))?;
        let idx = l.query(key, &b);
        let polys: Vec<Value> = idx
            .iter()
            .take(limit)
            .map(|&i| json!({"reference": format!("poly:{}/{}#{}", key.0, key.1, i), "cell": ld.owners.get(i), "bbox": ld.bboxes[i], "vertices": ld.polygons[i].len(), "area_um2": gamma_core::geom::signed_area(&ld.polygons[i]).abs()}))
            .collect();
        Ok(Json(json!({"count": idx.len(), "polygons": polys})))
    })
    .await
}

async fn stack_get(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    Ok(Json(json!(ps.stack().as_deref())))
}

async fn stack_put(State(hub): State<Hub>, Path(id): Path<String>, headers: HeaderMap, Json(s): Json<gamma_core::Stack>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let p = ps.snapshot();
    let path = p.manifest.layout.as_ref().map(|l| p.path(&l.stack)).unwrap_or_else(|| p.root.join("stack.toml"));
    gamma_core::fsutil::atomic_write(&path, s.to_toml().as_bytes())?;
    ps.invalidate_stack();
    ps.log.push(&actor(&headers), "stack.changed", json!({}));
    Ok(Json(json!(s)))
}

async fn sims(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let p = ps.snapshot();
    let v: Vec<Value> = p
        .manifest
        .simulations
        .iter()
        .map(|s| json!({"name": s.name, "spec": s.spec, "generator": s.generator, "device": s.device, "description": s.description, "built": p.path(&s.spec).is_file(), "preview": s.preview.is_some()}))
        .collect();
    Ok(Json(json!(v)))
}

fn preview(q: &HashMap<String, String>) -> bool {
    q.get("preview").map(|v| v == "1" || v == "true").unwrap_or(false)
}

async fn sim_setup(State(hub): State<Hub>, Path((id, name)): Path<(String, String)>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let pv = preview(&q);
    let devices = hub.devices();
    let mut remotes: Vec<Value> = hub.remotes(Some(&ps)).iter().map(|r| json!({"name": r.name, "ssh": r.ssh})).collect();
    let h2 = hub.clone();
    let mut s = blocking(move || {
        let sim = ps.sim(&name, pv)?;
        let mut v = serde_json::to_value(gamma_sim::setup::summarize(&name, &sim, &devices)?)?;
        // remote estimates from the throughput measured on each host's GPUs
        let tp = h2.throughput.lock().unwrap().clone();
        let cells = v["grid"]["cells"].as_f64().unwrap_or(0.0);
        let steps = v["grid"]["steps"].as_f64().unwrap_or(0.0);
        if let Some(est) = v["estimates"].as_array_mut() {
            for (k, t) in tp.iter().filter(|(k, _)| k.starts_with("remote:")) {
                est.push(json!({"device": k, "name": k, "seconds": cells * steps / t, "fits": null}));
            }
        }
        Ok(v)
    })
    .await?;
    s["remotes"] = json!(std::mem::take(&mut remotes));
    Ok(Json(s))
}

async fn sim_slice(State(hub): State<Hub>, Path((id, name)): Path<(String, String)>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let pv = preview(&q);
    let axis: usize = q.get("axis").and_then(|v| v.parse().ok()).unwrap_or(2);
    let coord: Option<f64> = q.get("coord").and_then(|v| v.parse().ok());
    let max: usize = q.get("max").and_then(|v| v.parse().ok()).unwrap_or(1600);
    blocking(move || {
        let sim = ps.sim(&name, pv)?;
        let c = coord.unwrap_or_else(|| {
            let d = gamma_sim::runner::SliceRequest::default_for(&sim);
            if d.axis == axis { d.coord } else { 0.5 * (sim.region.min[axis] + sim.region.max[axis]) }
        });
        let s = gamma_sim::setup::eps_slice(&sim, axis.min(2), c, max)?;
        let mut v = serde_json::to_value(&s)?;
        // pack the samples as base64 f32 to keep the response small
        use base64::Engine;
        let mut b = Vec::with_capacity(s.eps.len() * 4);
        for x in &s.eps {
            b.extend(x.to_le_bytes());
        }
        v["eps"] = json!(base64::engine::general_purpose::STANDARD.encode(b));
        let mut m = Vec::with_capacity(s.medium.len() * 2);
        for x in &s.medium {
            m.extend(x.to_le_bytes());
        }
        v["medium"] = json!(base64::engine::general_purpose::STANDARD.encode(m));
        Ok(Json(v))
    })
    .await
}

#[derive(Deserialize)]
struct RunReq {
    #[serde(default)]
    device: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    preview: bool,
    #[serde(default)]
    slice: Option<String>,
}

async fn sim_run(State(hub): State<Hub>, Path((id, name)): Path<(String, String)>, headers: HeaderMap, Json(r): Json<RunReq>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let cfg = ps.snapshot().sim(&name)?.clone();
    let mut device = r.device.or(cfg.device.clone()).unwrap_or_else(|| "cpu".into());
    let mut host = r.host.clone();
    // `remote:<host>/<device>` in the manifest or request
    if let Some(rest) = device.strip_prefix("remote:") {
        let (h, d) = rest.split_once('/').unwrap_or((rest, "cuda:0"));
        host = Some(h.to_string());
        device = d.to_string();
    }
    let host = match host {
        Some(h) => Some(hub.remote(&h, Some(&ps))?),
        None => {
            gamma_sim::devices::Device::parse(&device)?;
            None
        }
    };
    let job = hub.jobs.submit(ps.clone(), RunRequest { sim: name, device, host, preview: r.preview, actor: actor(&headers), slice: r.slice }, hub.worker.clone())?;
    // record measured throughput when it finishes
    let h2 = hub.clone();
    let jid = job.id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let Some(j) = h2.jobs.get(&jid) else { break };
            match j.status {
                crate::jobs::Status::Done => {
                    if let Some(p) = &j.progress {
                        if let Some(r) = p["mcells_per_s"].as_f64() {
                            let key = match &j.host {
                                Some(h) => format!("remote:{h}/{}", j.device),
                                None => h2.devices.iter().find(|d| d.id == j.device).map(|d| d.name.clone()).unwrap_or(j.device.clone()),
                            };
                            h2.record_throughput(&key, r * 1e6);
                        }
                    }
                    break;
                }
                crate::jobs::Status::Failed | crate::jobs::Status::Cancelled => break,
                _ => {}
            }
        }
    });
    Ok(Json(json!(job)))
}

async fn runs(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let sim = q.get("sim").cloned();
    Ok(Json(json!(blocking(move || Ok(ps.runs(sim.as_deref()))).await?)))
}

async fn run_view(State(hub): State<Hub>, Path((id, sim, run, what)): Path<(String, String, String, String)>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    blocking(move || {
        use gamma_sim::results as r;
        let spec = ps.run_spec(&sim, &run)?;
        let path = ps.run_dir(&sim, &run)?.join("results.json");
        let l = r::Loaded::read(&path, &spec)?;
        let mon = || q.get("monitor").cloned().ok_or_else(|| anyhow::anyhow!("monitor required"));
        let fi: usize = q.get("f").and_then(|v| v.parse().ok()).unwrap_or(0);
        Ok(Json(match what.as_str() {
            "overview" => json!(r::overview(&l)?),
            "spectrum" => json!(r::spectrum(&l, &mon()?)?),
            "sparams" => json!(r::sparams(&l)?),
            "field" => {
                let f = r::field_map(&l, &mon()?, q.get("q").map(|s| s.as_str()).unwrap_or("|E|2"), fi)?;
                let mut v = serde_json::to_value(&f)?;
                use base64::Engine;
                let mut b = Vec::with_capacity(f.data.len() * 4);
                for x in &f.data {
                    b.extend(x.to_le_bytes());
                }
                v["data"] = json!(base64::engine::general_purpose::STANDARD.encode(b));
                v
            }
            "time" => json!(r::time_series(&l, &mon()?)?),
            "decay" => json!(r::energy_decay(&l)),
            "farfield" => json!(r::far_field(&l, &mon()?, fi)?),
            other => anyhow::bail!("unknown result view '{other}'"),
        }))
    })
    .await
}

async fn check(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    blocking(move || {
        let p = ps.snapshot();
        let layout = ps.layout().ok().flatten();
        let stack = ps.stack();
        let mut f = gamma_core::checks::project_checks(&p, layout.as_deref(), stack.as_deref());
        if let (Some(l), Some(s)) = (&layout, &stack) {
            f.extend(gamma_core::checks::drc(l, s));
        }
        for s in &p.manifest.simulations {
            if let Ok(sim) = ps.sim(&s.name, false) {
                f.extend(gamma_sim::checks::check(&s.name, &sim, layout.as_deref(), stack.as_deref()));
            }
        }
        let errors = f.iter().filter(|x| x.severity == gamma_core::checks::Severity::Error).count();
        ps.log.push("hub", "check.done", json!({"findings": f.len(), "errors": errors}));
        Ok(Json(json!({"findings": f, "errors": errors})))
    })
    .await
}

async fn render(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Response> {
    let ps = proj(&hub, &id)?;
    let view = q.get("view").cloned().unwrap_or_else(|| "layout".into());
    let h = hub.clone();
    let data = blocking(move || crate::views::render(&ps, &h.jobs, &view, &q)).await?;
    Ok(png(data))
}

async fn selection_get(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    Ok(Json(ps.selection.lock().unwrap().clone().unwrap_or(Value::Null)))
}

async fn selection_set(State(hub): State<Hub>, Path(id): Path<String>, headers: HeaderMap, Json(v): Json<Value>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    *ps.selection.lock().unwrap() = if v.is_null() { None } else { Some(v.clone()) };
    ps.log.push(&actor(&headers), "selection", v.clone());
    Ok(Json(v))
}

async fn checkpoints(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let root = ps.root();
    let list = blocking(move || crate::git::list(&root)).await?;
    let comments = ps.comments();
    Ok(Json(json!({"checkpoints": list, "comments": comments})))
}

#[derive(Deserialize)]
struct CkReq {
    message: Option<String>,
}

async fn checkpoint_create(State(hub): State<Hub>, Path(id): Path<String>, headers: HeaderMap, Json(c): Json<CkReq>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let a = actor(&headers);
    let root = ps.root();
    let msg = c.message.unwrap_or_else(|| "Checkpoint".into());
    let a2 = a.clone();
    let ck = blocking(move || crate::git::checkpoint(&root, &msg, &a2)).await?;
    if let Some(c) = &ck {
        let _ = crate::projects::save_checkpoint_layout(&ps, c.n);
        ps.log.push(&a, "checkpoint", json!(c));
    }
    Ok(Json(json!(ck)))
}

async fn checkpoint_diff(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let a: u32 = q.get("a").and_then(|v| v.parse().ok()).ok_or_else(|| bad("a=<checkpoint number> required"))?;
    let b: Option<u32> = q.get("b").and_then(|v| v.parse().ok());
    blocking(move || crate::projects::diff_checkpoints(&ps, a, b)).await.map(Json)
}

async fn comments(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    Ok(Json(json!(ps.comments())))
}

#[derive(Deserialize)]
struct CommentReq {
    text: String,
    object: Option<String>,
    checkpoint: Option<u32>,
}

async fn comment_add(State(hub): State<Hub>, Path(id): Path<String>, headers: HeaderMap, Json(c): Json<CommentReq>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    Ok(Json(ps.add_comment(&actor(&headers), &c.text, c.object.as_deref(), c.checkpoint)?))
}

async fn events(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let after: u64 = q.get("after").and_then(|v| v.parse().ok()).unwrap_or(0);
    let kinds: Option<Vec<String>> = q.get("kinds").map(|k| k.split(',').map(|s| s.to_string()).collect());
    let kr: Option<Vec<&str>> = kinds.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());
    let limit: usize = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(500);
    let mut ev = ps.log.since(after, kr.as_deref());
    if ev.len() > limit {
        ev.drain(0..ev.len() - limit);
    }
    Ok(Json(json!(ev)))
}

async fn ws(State(hub): State<Hub>, Path(id): Path<String>, Query(q): Q, upgrade: WebSocketUpgrade) -> R<Response> {
    let ps = proj(&hub, &id)?;
    let after: u64 = q.get("after").and_then(|v| v.parse().ok()).unwrap_or(u64::MAX);
    Ok(upgrade.on_upgrade(move |sock| ws_loop(sock, ps, after)))
}

async fn ws_loop(mut sock: WebSocket, ps: Arc<ProjectState>, after: u64) {
    let mut rx = ps.log.subscribe();
    let backlog = if after == u64::MAX { vec![] } else { ps.log.since(after, None) };
    let mut last = after.min(ps.log.last_seq());
    for e in backlog {
        last = e.seq;
        if sock.send(Message::Text(serde_json::to_string(&e).unwrap_or_default().into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Ok(e) => {
                    if e.seq != 0 && e.seq <= last { continue; }
                    if e.seq != 0 { last = e.seq; }
                    if sock.send(Message::Text(serde_json::to_string(&e).unwrap_or_default().into())).await.is_err() { return; }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // re-read the log from the last sequence number seen
                    for e in ps.log.since(last, None) {
                        last = e.seq;
                        if sock.send(Message::Text(serde_json::to_string(&e).unwrap_or_default().into())).await.is_err() { return; }
                    }
                }
                Err(_) => return,
            },
            m = sock.recv() => match m {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => return,
                _ => {}
            }
        }
    }
}

#[allow(dead_code)]
fn _ev(_: Event) {}

async fn agent_status(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    Ok(Json(json!({"session": hub.agents.status(&ps), "providers": crate::agent::providers()})))
}

#[derive(Deserialize)]
struct AgentStart {
    provider: Option<String>,
}

async fn agent_start(State(hub): State<Hub>, Path(id): Path<String>, Json(a): Json<AgentStart>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let cfg = hub.config.lock().unwrap().agent.clone();
    let provider = a.provider.or(cfg.provider).unwrap_or_else(|| "claude".into());
    hub.agents.stop(&ps).await?;
    Ok(Json(hub.agents.start(ps, &provider, &hub.worker, &cfg.args).await?))
}

#[derive(Deserialize)]
struct AgentSend {
    text: String,
}

async fn agent_send(State(hub): State<Hub>, Path(id): Path<String>, Json(a): Json<AgentSend>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    if hub.agents.status(&ps).is_null() {
        let cfg = hub.config.lock().unwrap().agent.clone();
        hub.agents.start(ps.clone(), cfg.provider.as_deref().unwrap_or("claude"), &hub.worker, &cfg.args).await?;
    }
    hub.agents.send(ps, &a.text, &hub.worker).await?;
    Ok(Json(json!({"sent": true})))
}

async fn agent_stop(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    hub.agents.stop(&ps).await?;
    Ok(Json(json!({"stopped": true})))
}

async fn circuits(State(hub): State<Hub>, Path(id): Path<String>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let p = ps.snapshot();
    let engine = gamma_quantum::ExternalEngine::default();
    use gamma_quantum::Engine;
    Ok(Json(json!({"circuits": p.manifest.circuits, "engine": engine.name(), "engine_available": engine.available()})))
}

async fn circuit(State(hub): State<Hub>, Path((id, name)): Path<(String, String)>) -> R<Json<Value>> {
    let ps = proj(&hub, &id)?;
    let p = ps.snapshot();
    let c = p.manifest.circuits.iter().find(|c| c.name == name).ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("no circuit {name}")))?;
    Ok(Json(json!(gamma_quantum::read_circuit(&p.path(&c.file))?)))
}
