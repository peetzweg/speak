//! `speak voices`: a tiny interactive picker (crossterm only; works in macOS, Linux and Windows
//! terminals). Up/Down to browse, Space to hear a sample, Enter to save as default, q/Esc to quit.

use std::io::{Write, stderr};
use std::sync::mpsc;

use anyhow::Result;
use crossterm::{
    cursor, event::{self, Event, KeyCode, KeyEventKind}, execute, queue,
    style::{Attribute, Print, SetAttribute}, terminal,
};

use crate::backend::{Backend, Voice};

pub fn run(mut backend: Box<dyn Backend>, current: &str) -> Result<Option<String>> {
    let voices: Vec<Voice> = backend.voices()?;
    let rate = backend.info().sample_rate;
    let mut sel = voices.iter().position(|v| v.id == current).unwrap_or(0);

    // Previews run on a worker that owns the backend; only the newest request is honoured.
    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        while let Ok(mut id) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                id = newer;
            }
            let name = id.split('_').nth(1).unwrap_or(&id).to_string();
            let mut pcm = Vec::new();
            if let Ok(chunks) = backend.synthesize(&format!("Hi, I'm {name}. This is how I sound."), &id, 1.0) {
                for c in chunks.flatten() {
                    pcm.extend(c.samples);
                }
            }
            let (ptx, prx) = mpsc::channel();
            let _ = ptx.send(pcm);
            drop(ptx);
            let _ = crate::audio::play(prx, rate);
        }
    });

    let mut out = stderr();
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
    let result = (|| -> Result<Option<String>> {
        loop {
            let (_, rows) = terminal::size()?;
            let page = rows.saturating_sub(3).max(1) as usize;
            let top = sel.saturating_sub(page / 2).min(voices.len().saturating_sub(page));
            queue!(out, terminal::Clear(terminal::ClearType::All), cursor::MoveTo(0, 0))?;
            queue!(out, Print("speak voices  ↑/↓ browse  space hear  enter set default  q quit\r\n\r\n"))?;
            for (i, v) in voices.iter().enumerate().skip(top).take(page) {
                if i == sel {
                    queue!(out, SetAttribute(Attribute::Reverse))?;
                }
                queue!(out, Print(format!(" {:<14} {}", v.id, v.label)), SetAttribute(Attribute::Reset), Print("\r\n"))?;
            }
            out.flush()?;
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press {
                    continue; // Windows reports press and release
                }
                match k.code {
                    KeyCode::Up | KeyCode::Char('k') => sel = sel.saturating_sub(1),
                    KeyCode::Down | KeyCode::Char('j') => sel = (sel + 1).min(voices.len() - 1),
                    KeyCode::Char(' ') | KeyCode::Char('p') => {
                        let _ = tx.send(voices[sel].id.clone());
                    }
                    KeyCode::Enter => return Ok(Some(voices[sel].id.clone())),
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                    _ => {}
                }
            }
        }
    })();
    execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    result
}
