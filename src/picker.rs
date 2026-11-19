//! `speak voices` on a terminal: browse voices grouped by language, space to hear a sample,
//! enter to save the default, q or Esc to quit. crossterm only, so it runs in macOS, Linux and
//! Windows terminals.

use std::io::{Write, stderr};
use std::sync::mpsc;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::style::{Attribute, Print, SetAttribute};
use crossterm::{cursor, execute, queue, terminal};

use crate::backend::{Backend, Voice};

enum Row<'a> {
    Header(&'a str),
    Voice(usize),
}

pub fn run(mut backend: Box<dyn Backend>, title: &str, current: &str) -> Result<Option<String>> {
    let voices: Vec<Voice> = backend.voices()?;
    if voices.is_empty() {
        return Ok(None);
    }
    let rate = backend.info().sample_rate;
    let mut rows = Vec::new();
    for (i, v) in voices.iter().enumerate() {
        if i == 0 || voices[i - 1].language != v.language {
            rows.push(Row::Header(&v.language));
        }
        rows.push(Row::Voice(i));
    }
    let row_of = |voice: usize| rows.iter().position(|r| matches!(r, Row::Voice(i) if *i == voice)).unwrap_or(0);
    let mut sel = voices.iter().position(|v| v.id == current).unwrap_or(0);

    // Samples are synthesised on a worker that owns the backend; only the newest request plays.
    let (tx, rx) = mpsc::channel::<String>();
    let worker = std::thread::spawn(move || {
        while let Ok(mut id) = rx.recv() {
            while let Ok(newer) = rx.try_recv() {
                id = newer;
            }
            let name = id.split('_').nth(1).unwrap_or(&id).to_string();
            let mut pcm = Vec::new();
            if let Ok(chunks) = backend.synthesize(&format!("Hi, I'm {name}. This is how I sound."), &id, 1.0) {
                pcm.extend(chunks.flatten().flat_map(|c| c.samples));
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
            let (_, height) = terminal::size()?;
            let page = height.saturating_sub(3).max(1) as usize;
            let at = row_of(sel);
            let top = at.saturating_sub(page / 2).min(rows.len().saturating_sub(page));
            queue!(out, terminal::Clear(terminal::ClearType::All), cursor::MoveTo(0, 0))?;
            queue!(out, Print(format!("speak voices ({title})  ↑/↓ browse  space hear  enter set default  q quit\r\n\r\n")))?;
            for row in rows.iter().skip(top).take(page) {
                match row {
                    Row::Header(lang) => {
                        queue!(out, SetAttribute(Attribute::Bold), Print(format!(" {lang}")), SetAttribute(Attribute::Reset))?
                    }
                    Row::Voice(i) => {
                        let v = &voices[*i];
                        let mark = if v.id == current { "*" } else { " " };
                        if *i == sel {
                            queue!(out, SetAttribute(Attribute::Reverse))?;
                        }
                        queue!(out, Print(format!("  {mark} {:<14} {}", v.id, v.label)), SetAttribute(Attribute::Reset))?;
                    }
                }
                queue!(out, Print("\r\n"))?;
            }
            out.flush()?;
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press {
                    continue; // Windows reports both press and release
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
    // Let a sample finish and the backend shut down before the process exits.
    drop(tx);
    let _ = worker.join();
    result
}
