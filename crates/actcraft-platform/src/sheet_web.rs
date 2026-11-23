//! Sheets in the browser: musicians on the local network open a page on a tablet or laptop, sign
//! in once with a six-digit code ActCraft shows, pick their name and see the sheet made for them
//! for the song that plays, turning pages as the song goes on.
//!
//! It is off until switched on, and only addresses on the local network are let in. The pages are
//! plain HTML written here, without scripts: a page asks for itself again at once, and the answer
//! waits until there is something new to show (a new song, sheet or page), so the browser keeps
//! the page it has until then. PDF pages are drawn here and sent as PNG pictures. Requests are
//! read with bounds (head, body, connections) and nothing from a request names a file: the only
//! files read are the sheets of the show.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::band::is_local;

/// The port the pages are served on.
pub const SHEETS_PORT: u16 = 47392;
/// Connections at once.
const MAX_CONNECTIONS: usize = 48;
/// Longest request line or header line, and most header lines.
const MAX_LINE: u64 = 4096;
const MAX_HEADERS: usize = 64;
/// Longest form sent (the code).
const MAX_BODY: usize = 256;
/// How long a page waits for something new before it is sent as it is.
const HOLD: Duration = Duration::from_secs(25);
/// Wrong codes before the code changes.
const MAX_WRONG: usize = 5;
/// Width of a drawn PDF page, pixels.
const PAGE_WIDTH: u32 = 1240;
/// Pictures and page counts kept.
const MAX_PICTURES: usize = 24;
/// Largest PDF read.
const MAX_PDF_BYTES: usize = 64 << 20;
const COOKIE: &str = "actcraft_sheets";
/// Asked of the translator for the pages' language code.
const LANG: &str = "\0lang";

/// Turns a window text into the window's language.
pub type Translate = Arc<dyn Fn(&str) -> String + Send + Sync>;

/// A page of a PDF file: the file and the page (from 0).
type PageOf = (String, usize);

/// What the server's threads share.
#[derive(Default)]
struct Shared {
    code: String,
    wrong: usize,
    tokens: Vec<String>,
    new_tokens: Vec<String>,
    /// The engine's `sheets_state`.
    state: Value,
    /// Pages of each PDF read so far.
    page_counts: HashMap<String, usize>,
    /// Drawn pages, newest last: (file, page) and the PNG.
    pictures: Vec<(PageOf, Arc<Vec<u8>>)>,
    /// Per musician, the picture of the score lines playing now (SVG) and its key.
    scores: Vec<Option<(String, Arc<String>)>>,
}

/// The page server, while it lives.
pub struct SheetWeb {
    addr: SocketAddr,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    connections: Arc<AtomicUsize>,
    quit: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SheetWeb {
    /// Listens on `port` on every network of the computer (only local addresses are let in),
    /// knowing the tokens of browsers signed in before.
    /// `lang` is the pages' language code ("en", "de"), `tr` turns their texts into it.
    pub fn open(port: u16, tokens: Vec<String>, lang: &str, tr: Translate) -> Result<Self, String> {
        Self::open_on(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)), tokens, lang, tr)
    }

    fn open_on(at: SocketAddr, tokens: Vec<String>, lang: &str, tr: Translate) -> Result<Self, String> {
        let lang: String = lang.chars().filter(char::is_ascii_alphanumeric).take(8).collect();
        let tr: Translate = Arc::new(move |s: &str| if s == LANG { lang.clone() } else { tr(s) });
        let listener = TcpListener::bind(at).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let shared = Arc::new((
            Mutex::new(Shared { code: crate::band::new_code()?, tokens, state: Value::Null, ..Shared::default() }),
            Condvar::new(),
        ));
        let connections = Arc::new(AtomicUsize::new(0));
        let quit = Arc::new(AtomicBool::new(false));
        let (s, c, q) = (Arc::clone(&shared), Arc::clone(&connections), Arc::clone(&quit));
        let thread = std::thread::Builder::new()
            .name("actcraft-sheets-web".into())
            .spawn(move || {
                while !q.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, from)) if is_local(from.ip()) && c.load(Ordering::Relaxed) < MAX_CONNECTIONS => {
                            c.fetch_add(1, Ordering::Relaxed);
                            let (s, count, q, tr) = (Arc::clone(&s), Arc::clone(&c), Arc::clone(&q), Arc::clone(&tr));
                            let spawned =
                                std::thread::Builder::new().name("actcraft-sheets-page".into()).spawn(move || {
                                    answer(stream, &s, &q, &tr);
                                    count.fetch_sub(1, Ordering::Relaxed);
                                });
                            if spawned.is_err() {
                                c.fetch_sub(1, Ordering::Relaxed);
                            }
                        }
                        // Not from the local network, or too many: closed at once.
                        Ok(_) => {}
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { addr, shared, connections, quit, thread: Some(thread) })
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// The code a new browser signs in with.
    pub fn code(&self) -> String {
        self.shared.0.lock().map(|s| s.code.clone()).unwrap_or_default()
    }

    /// What the pages show from now on (the engine's `sheets_state`); waiting pages look again.
    pub fn publish(&self, state: Value) {
        if let Ok(mut s) = self.shared.0.lock()
            && s.state != state
        {
            s.state = state;
            self.shared.1.notify_all();
        }
    }

    /// The score picture musician `index` sees now (`None`: none); waiting pages look again when
    /// its key changes.
    pub fn publish_score(&self, index: usize, picture: Option<(String, String)>) {
        if index >= 256 {
            return;
        }
        if let Ok(mut s) = self.shared.0.lock() {
            if s.scores.len() <= index {
                s.scores.resize(index + 1, None);
            }
            let same = match (s.scores.get(index).and_then(Option::as_ref), &picture) {
                (Some((old, _)), Some((new, _))) => old == new,
                (None, None) => true,
                _ => false,
            };
            if !same {
                if let Some(slot) = s.scores.get_mut(index) {
                    *slot = picture.map(|(k, svg)| (k, Arc::new(svg)));
                }
                self.shared.1.notify_all();
            }
        }
    }

    /// Tokens of browsers signed in since the last call (to keep on this computer).
    pub fn take_new_tokens(&self) -> Vec<String> {
        self.shared.0.lock().map(|mut s| std::mem::take(&mut s.new_tokens)).unwrap_or_default()
    }

    /// Pages being asked for or waited on now: about one per open browser.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::Relaxed)
    }
}

impl Drop for SheetWeb {
    fn drop(&mut self) {
        self.quit.store(true, Ordering::Relaxed);
        self.shared.1.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// This computer's address on the local network (the one a route to a private address would
/// use; nothing is sent), for the address musicians type.
pub fn local_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 168, 0, 1), 9)).ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && is_local(ip)).then_some(ip)
}

/// A request: method, path, query, the sign-in cookie and the form body.
#[derive(Debug, Default, PartialEq)]
struct Request {
    method: String,
    path: String,
    query: String,
    token: Option<String>,
    body: String,
}

fn read_request(reader: &mut impl BufRead) -> Option<Request> {
    let mut line = String::new();
    reader.take(MAX_LINE).read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut request = Request { method, path: path.to_owned(), query: query.to_owned(), ..Request::default() };
    let mut length = 0usize;
    for _ in 0..MAX_HEADERS {
        let mut header = String::new();
        reader.take(MAX_LINE).read_line(&mut header).ok()?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value.parse().ok()?;
        } else if name.eq_ignore_ascii_case("cookie") {
            request.token = value
                .split(';')
                .filter_map(|c| c.trim().split_once('='))
                .find(|(k, _)| *k == COOKIE)
                .map(|(_, v)| v.to_owned());
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    request.body = String::from_utf8_lossy(&body).into_owned();
    Some(request)
}

/// A query or form field.
fn field<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').filter_map(|kv| kv.split_once('=')).find(|(k, _)| *k == name).map(|(_, v)| v)
}

/// Text for HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out += "&amp;",
            '<' => out += "&lt;",
            '>' => out += "&gt;",
            '"' => out += "&quot;",
            '\'' => out += "&#39;",
            c => out.push(c),
        }
    }
    out
}

/// A whole page: dark around white paper, large text, the language of the window.
fn html(tr: &Translate, title: &str, body: &str, refresh: Option<&str>) -> String {
    let refresh = refresh.map(|to| format!("<meta http-equiv=\"refresh\" content=\"0;url={to}\">")).unwrap_or_default();
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">{refresh}<title>{title}</title><style>body{{margin:0;background:#141414;color:#f2f2f2;font:20px/1.4 system-ui,sans-serif}}main{{padding:12px}}h1{{font-size:24px;margin:8px 0}}a{{color:#8cc8ff}}img{{display:block;width:100%;height:auto;background:#fff;margin:0 0 8px}}input,button{{font-size:24px;padding:8px}}ul{{list-style:none;padding:0}}li a{{display:block;padding:14px;margin:6px 0;background:#262626;border-radius:6px;text-decoration:none}}.note{{color:#bdbdbd}}</style></head><body><main>{body}</main></body></html>",
        lang = escape(&tr(LANG)),
        title = escape(title),
    )
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, extra: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; form-action 'self'\r\n{extra}\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn respond_html(stream: &mut TcpStream, page: &str) {
    respond(stream, "200 OK", "text/html; charset=utf-8", "Cache-Control: no-store\r\n", page.as_bytes());
}

/// One connection: one request and its answer.
fn answer(stream: TcpStream, shared: &(Mutex<Shared>, Condvar), quit: &AtomicBool, tr: &Translate) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let Ok(read_half) = stream.try_clone() else { return };
    let mut stream = stream;
    let Some(request) = read_request(&mut BufReader::new(read_half)) else {
        respond(&mut stream, "400 Bad Request", "text/plain", "", b"bad request");
        return;
    };
    let signed_in = request
        .token
        .as_ref()
        .is_some_and(|t| !t.is_empty() && shared.0.lock().is_ok_and(|s| s.tokens.iter().any(|k| k == t)));
    match (request.method.as_str(), request.path.as_str()) {
        ("POST", "/pair") => pair(&mut stream, shared, tr, &request.body),
        ("GET", "/") if signed_in => respond_html(&mut stream, &names_page(shared, tr)),
        ("GET", "/") => respond_html(&mut stream, &pair_page(tr, false)),
        ("GET", path) if signed_in && path.starts_with("/m/") => {
            let Some(index) = path.strip_prefix("/m/").and_then(|i| i.parse::<usize>().ok()) else {
                return respond(&mut stream, "404 Not Found", "text/plain", "", b"not found");
            };
            let page = musician_page(shared, quit, tr, index, field(&request.query, "k"));
            respond_html(&mut stream, &page);
        }
        ("GET", path) if signed_in && path.starts_with("/p/") => picture(&mut stream, shared, path),
        ("GET", path) if signed_in && path.starts_with("/s/") => score_picture(&mut stream, shared, path),
        ("GET", _) if !signed_in => {
            respond(&mut stream, "303 See Other", "text/plain", "Location: /\r\n", b"");
        }
        _ => respond(&mut stream, "404 Not Found", "text/plain", "", b"not found"),
    }
}

fn pair_page(tr: &Translate, wrong: bool) -> String {
    let note = if wrong {
        format!("<p class=\"note\">{}</p>", escape(&tr("That code is not right. Look again in ActCraft.")))
    } else {
        String::new()
    };
    let body = format!(
        "<h1>ActCraft</h1><p>{}</p>{note}<form method=\"post\" action=\"/pair\"><label for=\"code\">{}</label><br><input id=\"code\" name=\"code\" inputmode=\"numeric\" autocomplete=\"off\" maxlength=\"6\" autofocus> <button type=\"submit\">{}</button></form>",
        escape(&tr("Enter the code ActCraft shows under Settings > Band.")),
        escape(&tr("Code")),
        escape(&tr("Sign in")),
    );
    html(tr, &tr("Sheets"), &body, None)
}

fn pair(stream: &mut TcpStream, shared: &(Mutex<Shared>, Condvar), tr: &Translate, body: &str) {
    let given = field(body, "code").unwrap_or_default().trim().to_owned();
    let token = {
        let Ok(mut s) = shared.0.lock() else { return };
        if !given.is_empty() && given == s.code {
            let token = crate::band::random_hex(16).unwrap_or_default();
            if !token.is_empty() {
                s.tokens.push(token.clone());
                s.new_tokens.push(token.clone());
            }
            Some(token)
        } else {
            s.wrong += 1;
            if s.wrong >= MAX_WRONG {
                s.wrong = 0;
                s.code = crate::band::new_code().unwrap_or_default();
            }
            None
        }
    };
    // Each try takes a moment, so codes can't be guessed quickly.
    std::thread::sleep(Duration::from_millis(400));
    match token {
        Some(t) if !t.is_empty() => {
            let extra = format!(
                "Location: /\r\nSet-Cookie: {COOKIE}={t}; Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict\r\n"
            );
            respond(stream, "303 See Other", "text/plain", &extra, b"");
        }
        _ => respond_html(stream, &pair_page(tr, true)),
    }
}

fn names_page(shared: &(Mutex<Shared>, Condvar), tr: &Translate) -> String {
    let names: Vec<String> = shared
        .0
        .lock()
        .ok()
        .and_then(|s| s.state.get("musicians").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .iter()
        .map(|m| m.get("name").and_then(Value::as_str).unwrap_or_default().to_owned())
        .collect();
    let body = if names.is_empty() {
        format!(
            "<h1>{}</h1><p>{}</p>",
            escape(&tr("Who are you?")),
            escape(&tr("No musicians yet: add them in ActCraft's Sheet music tab."))
        )
    } else {
        let items: String =
            names.iter().enumerate().map(|(i, n)| format!("<li><a href=\"/m/{i}\">{}</a></li>", escape(n))).collect();
        format!("<h1>{}</h1><ul>{items}</ul>", escape(&tr("Who are you?")))
    };
    html(tr, &tr("Sheets"), &body, None)
}

/// What a musician's page shows now.
#[derive(Debug, Clone, PartialEq)]
struct View {
    name: String,
    song: Option<String>,
    sheet: Option<String>,
    pdf: Option<String>,
    score: bool,
    /// The page in view and the page count (0 when the PDF can't be read).
    page: usize,
    pages: usize,
    /// The key of the score picture shown, for a score sheet.
    score_key: Option<String>,
}

impl View {
    /// Changes whenever the page should change.
    fn key(&self) -> String {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let text = format!(
            "{:?}|{:?}|{:?}|{}|{}|{}|{:?}",
            self.song, self.sheet, self.pdf, self.score, self.page, self.pages, self.score_key
        );
        for b in text.bytes() {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x100_0000_01b3);
        }
        format!("{hash:016x}")
    }
}

fn view_of(shared: &(Mutex<Shared>, Condvar), index: usize) -> Option<View> {
    let (state, counts, score_key) = {
        let s = shared.0.lock().ok()?;
        let key = s.scores.get(index).and_then(Option::as_ref).map(|(k, _)| k.clone());
        (s.state.clone(), s.page_counts.clone(), key)
    };
    let musician = state.get("musicians")?.as_array()?.get(index)?;
    let sheet = musician.get("sheet").filter(|s| !s.is_null());
    let pdf = sheet.and_then(|s| s.get("pdf_file")).and_then(Value::as_str).map(str::to_owned);
    let pages = match &pdf {
        Some(p) => match counts.get(p) {
            Some(n) => *n,
            None => {
                let n = page_count(p);
                if let Ok(mut s) = shared.0.lock() {
                    if s.page_counts.len() >= MAX_PICTURES {
                        s.page_counts.clear();
                    }
                    s.page_counts.insert(p.clone(), n);
                }
                n
            }
        },
        None => 0,
    };
    let progress = state.get("progress").and_then(Value::as_f64).filter(|p| p.is_finite()).unwrap_or(0.0);
    let page = if pages == 0 { 0 } else { ((progress * pages as f64) as usize).min(pages - 1) };
    Some(View {
        name: musician.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
        song: state.get("song").and_then(Value::as_str).map(str::to_owned),
        sheet: sheet.and_then(|s| s.get("name")).and_then(Value::as_str).map(str::to_owned),
        score: pdf.is_none() && sheet.and_then(|s| s.get("notation_file")).is_some_and(|n| !n.is_null()),
        score_key: score_key.filter(|_| pdf.is_none()),
        pdf,
        page,
        pages,
    })
}

/// The musician's page, once it differs from the one the browser has (`known`), or after a while.
fn musician_page(
    shared: &(Mutex<Shared>, Condvar),
    quit: &AtomicBool,
    tr: &Translate,
    index: usize,
    known: Option<&str>,
) -> String {
    let until = Instant::now() + HOLD;
    let mut view = view_of(shared, index);
    while let (Some(v), Some(k)) = (&view, known) {
        if v.key() != k || Instant::now() >= until || quit.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(guard) = shared.0.lock() {
            let _ = shared.1.wait_timeout(guard, Duration::from_millis(500));
        }
        view = view_of(shared, index);
    }
    let Some(view) = view else {
        return html(tr, &tr("Sheets"), &format!("<p><a href=\"/\">{}</a></p>", escape(&tr("Pick your name"))), None);
    };
    let mut body = format!(
        "<h1>{}</h1><p class=\"note\">{} &middot; <a href=\"/\">{}</a></p>",
        escape(view.song.as_deref().unwrap_or_default()),
        escape(&view.name),
        escape(&tr("Not you?")),
    );
    match (&view.pdf, view.score) {
        (Some(file), _) if view.pages > 0 => {
            let id = file_id(file);
            for page in view.page..(view.page + 2).min(view.pages) {
                let alt = tr("{sheet}, page {page} of {pages}")
                    .replace("{sheet}", view.sheet.as_deref().unwrap_or_default())
                    .replace("{page}", &(page + 1).to_string())
                    .replace("{pages}", &view.pages.to_string());
                body.push_str(&format!("<img src=\"/p/{index}/{id}/{page}\" alt=\"{}\">", escape(&alt)));
            }
        }
        (Some(_), _) => body.push_str(&format!("<p>{}</p>", escape(&tr("The PDF could not be opened.")))),
        (None, true) => match &view.score_key {
            Some(key) => {
                let alt =
                    tr("{sheet}, the lines playing now").replace("{sheet}", view.sheet.as_deref().unwrap_or_default());
                body.push_str(&format!("<img src=\"/s/{index}/{key}\" alt=\"{}\">", escape(&alt)));
            }
            None => body.push_str(&format!("<p>{}</p>", escape(&tr("The score is being drawn.")))),
        },
        (None, false) if view.song.is_some() => {
            body.push_str(&format!("<p>{}</p>", escape(&tr("No sheet for you in this song."))));
        }
        _ => body.push_str(&format!("<p>{}</p>", escape(&tr("Nothing is playing.")))),
    }
    let again = format!("/m/{index}?k={}", view.key());
    let title = format!("{} \u{2014} {}", view.name, tr("Sheets"));
    html(tr, &title, &body, Some(&again))
}

/// A short id for a file, so its pictures are cached apart from another file's.
fn file_id(file: &str) -> String {
    View {
        name: String::new(),
        song: None,
        sheet: None,
        pdf: Some(file.to_owned()),
        score: false,
        page: 0,
        pages: 0,
        score_key: None,
    }
    .key()
}

/// `/s/<musician>/<key>`: the score lines that musician sees now, as SVG.
fn score_picture(stream: &mut TcpStream, shared: &(Mutex<Shared>, Condvar), path: &str) {
    let mut parts = path.trim_start_matches("/s/").split('/');
    let (Some(index), Some(key)) = (parts.next().and_then(|i| i.parse::<usize>().ok()), parts.next()) else {
        return respond(stream, "404 Not Found", "text/plain", "", b"not found");
    };
    let svg = shared.0.lock().ok().and_then(|s| {
        s.scores.get(index).and_then(Option::as_ref).filter(|(k, _)| k == key).map(|(_, svg)| Arc::clone(svg))
    });
    match svg {
        Some(svg) => respond(
            stream,
            "200 OK",
            "image/svg+xml",
            "Cache-Control: private, max-age=3600
",
            svg.as_bytes(),
        ),
        None => respond(stream, "404 Not Found", "text/plain", "", b"not found"),
    }
}

/// `/p/<musician>/<file id>/<page>`: a page of that musician's PDF as a PNG.
fn picture(stream: &mut TcpStream, shared: &(Mutex<Shared>, Condvar), path: &str) {
    let mut parts = path.trim_start_matches("/p/").split('/');
    let (Some(index), Some(id), Some(page)) = (
        parts.next().and_then(|i| i.parse::<usize>().ok()),
        parts.next(),
        parts.next().and_then(|p| p.parse::<usize>().ok()),
    ) else {
        return respond(stream, "404 Not Found", "text/plain", "", b"not found");
    };
    // Only the file the musician's sheet names now, and only one of its pages.
    let Some(file) = view_of(shared, index).and_then(|v| v.pdf.filter(|f| file_id(f) == id && page < v.pages)) else {
        return respond(stream, "404 Not Found", "text/plain", "", b"not found");
    };
    let cached = shared
        .0
        .lock()
        .ok()
        .and_then(|s| s.pictures.iter().find(|(k, _)| k.0 == file && k.1 == page).map(|(_, png)| Arc::clone(png)));
    let png = match cached {
        Some(png) => png,
        None => {
            let Some(png) = draw_page(&file, page).map(Arc::new) else {
                return respond(stream, "404 Not Found", "text/plain", "", b"not found");
            };
            if let Ok(mut s) = shared.0.lock() {
                if s.pictures.len() >= MAX_PICTURES {
                    s.pictures.remove(0);
                }
                s.pictures.push(((file, page), Arc::clone(&png)));
            }
            png
        }
    };
    respond(stream, "200 OK", "image/png", "Cache-Control: private, max-age=3600\r\n", &png);
}

fn read_pdf(file: &str) -> Option<Vec<u8>> {
    let bytes = std::fs::read(file).ok()?;
    (bytes.len() <= MAX_PDF_BYTES).then_some(bytes)
}

fn page_count(file: &str) -> usize {
    read_pdf(file).and_then(|b| craft_pdf::syntax::Pdf::new(b).ok()).map_or(0, |pdf| pdf.pages().len())
}

/// Draws one page of a PDF as a PNG, white paper.
fn draw_page(file: &str, index: usize) -> Option<Vec<u8>> {
    let pdf = craft_pdf::syntax::Pdf::new(read_pdf(file)?).ok()?;
    let pages = pdf.pages();
    let page = pages.get(index)?;
    let (w, _) = page.render_dimensions();
    let scale = PAGE_WIDTH as f32 / w.max(1.0);
    let cache = craft_pdf::raster::RenderCache::new();
    let settings = craft_pdf::interpret::InterpreterSettings::default();
    let options =
        craft_pdf::raster::Options { scale: [scale, scale], background: [255, 255, 255, 255], ..Default::default() };
    let raster = craft_pdf::raster::render(page, &cache, &settings, &options).ok()?;
    let mut out = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut out),
        raster.bytes(),
        raster.width,
        raster.height,
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tr() -> Translate {
        Arc::new(|s: &str| s.to_owned())
    }

    /// One request to the server, the whole answer back.
    fn ask(addr: SocketAddr, request: &str) -> String {
        let Ok(mut s) = TcpStream::connect(addr) else { return String::new() };
        let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
        let _ = s.write_all(request.as_bytes());
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    #[test]
    fn a_browser_signs_in_with_the_code_and_follows_its_sheet() {
        let web = SheetWeb::open_on(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Vec::new(), "en", tr());
        let Ok(web) = web else { return };
        let addr = web.addr;
        web.publish(json!({"song": "Opener", "progress": 0.0, "playing": true, "musicians": [
            {"name": "Kim <Bass>", "sheet": {"song": 1, "index": 0, "name": "Bass", "pdf_file": null, "notation_file": "/x.mid"}},
            {"name": "Alex", "sheet": null}]}));
        // Not signed in: the code form, and nothing else.
        assert!(ask(addr, "GET / HTTP/1.1\r\n\r\n").contains("name=\"code\""));
        assert!(ask(addr, "GET /m/0 HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 303"));
        let wrong = ask(addr, "POST /pair HTTP/1.1\r\nContent-Length: 11\r\n\r\ncode=000000");
        let code = web.code();
        let body = format!("code={code}");
        let right = ask(addr, &format!("POST /pair HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}", body.len()));
        if code != "000000" {
            assert!(wrong.contains("not right"));
        }
        let token =
            right.split("actcraft_sheets=").nth(1).and_then(|t| t.split(';').next()).unwrap_or_default().to_owned();
        assert_eq!(token.len(), 32, "{right}");
        assert_eq!(web.take_new_tokens(), vec![token.clone()]);
        let cookie = format!("Cookie: other=1; {COOKIE}={token}\r\n");
        // The names, escaped; then a musician's page.
        let names = ask(addr, &format!("GET / HTTP/1.1\r\n{cookie}\r\n"));
        assert!(names.contains("Kim &lt;Bass&gt;") && names.contains("href=\"/m/1\""));
        let kim = ask(addr, &format!("GET /m/0 HTTP/1.1\r\n{cookie}\r\n"));
        assert!(kim.contains("Opener") && kim.contains("being drawn"));
        // Once its picture is there, the score shows as that picture, and only by its key.
        web.publish_score(0, Some(("k1".into(), "<svg xmlns=\"http://www.w3.org/2000/svg\"/>".into())));
        let kim = ask(addr, &format!("GET /m/0 HTTP/1.1\r\n{cookie}\r\n"));
        assert!(kim.contains("src=\"/s/0/k1\"") && kim.contains("Bass, the lines playing now"));
        let svg = ask(addr, &format!("GET /s/0/k1 HTTP/1.1\r\n{cookie}\r\n"));
        assert!(svg.contains("image/svg+xml") && svg.ends_with("/>"));
        assert!(ask(addr, &format!("GET /s/0/k0 HTTP/1.1\r\n{cookie}\r\n")).starts_with("HTTP/1.1 404"));
        let alex = ask(addr, &format!("GET /m/1 HTTP/1.1\r\n{cookie}\r\n"));
        assert!(alex.contains("No sheet for you"));
        // A page that knows the view waits until it changes.
        let key = alex.split("k=").nth(1).and_then(|k| k.split('"').next()).unwrap_or_default().to_owned();
        let started = Instant::now();
        let waiter = std::thread::spawn(move || ask(addr, &format!("GET /m/1?k={key} HTTP/1.1\r\n{cookie}\r\n")));
        std::thread::sleep(Duration::from_millis(300));
        web.publish(json!({"song": "Ballad", "progress": 0.0, "playing": true, "musicians": [
            {"name": "Kim", "sheet": null}, {"name": "Alex", "sheet": null}]}));
        let next = waiter.join().unwrap_or_default();
        assert!(next.contains("Ballad"));
        assert!(started.elapsed() < Duration::from_secs(5), "answered when the song changed");
        // No path names a file; bad requests are refused.
        assert!(
            ask(addr, &format!("GET /p/0/x/0 HTTP/1.1\r\nCookie: {COOKIE}={token}\r\n\r\n"))
                .starts_with("HTTP/1.1 404")
        );
        assert!(ask(addr, "POST /pair HTTP/1.1\r\nContent-Length: 99999\r\n\r\n").starts_with("HTTP/1.1 400"));
    }

    #[test]
    fn fields_and_text_are_read_and_written_safely() {
        assert_eq!(field("a=1&k=abc", "k"), Some("abc"));
        assert_eq!(field("a=1", "k"), None);
        assert_eq!(escape("<a href=\"x\">'&"), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;");
        let request =
            read_request(&mut BufReader::new(&b"GET /m/2?k=ff HTTP/1.1\r\nCookie: actcraft_sheets=ab\r\n\r\n"[..]));
        assert_eq!(
            request,
            Some(Request {
                method: "GET".into(),
                path: "/m/2".into(),
                query: "k=ff".into(),
                token: Some("ab".into()),
                body: String::new()
            })
        );
        assert!(read_request(&mut BufReader::new(&b""[..])).is_none());
    }
}
