use std::fs;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};
use cursive::theme::{Error, load_theme_file, load_toml, Theme};
use dirs::config_dir;
use rusqlite::{Connection, Row};
use serde_derive::{Deserialize, Serialize};
#[cfg(feature = "i18n")]
use crate::i18n;
use crate::{Asset, package_name};
use crate::common::Position;
use crate::terminal::Listable;

pub struct ReadingInfo {
	row_id: i64,
	pub filename: String,
	pub inner_book: usize,
	pub chapter: usize,
	pub line: usize,
	pub position: usize,
	pub custom_color: bool,
	pub strip_empty_lines: bool,
}

impl ReadingInfo {
	#[inline]
	pub fn new(filename: &str) -> Self
	{
		ReadingInfo {
			row_id: 0,
			filename: String::from(filename),
			inner_book: 0,
			chapter: 0,
			line: 0,
			position: 0,
			custom_color: true,
			strip_empty_lines: false,
		}
	}
	#[inline]
	pub(crate) fn with_last_chapter(mut self) -> Self
	{
		self.chapter = usize::MAX;
		self
	}
	#[inline]
	pub(crate) fn with_inner_book(mut self, inner_book: usize) -> Self
	{
		self.inner_book = inner_book;
		self.chapter = 0;
		self.line = 0;
		self.position = 0;
		self
	}
	#[inline]
	#[allow(unused)]
	pub(crate) fn no_custom_color(mut self) -> Self
	{
		self.custom_color = false;
		self
	}
	#[inline]
	#[allow(unused)]
	pub(crate) fn pos(&self) -> Position
	{
		Position::new(self.line, self.position)
	}

	fn now() -> u64
	{
		SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.unwrap()
			.as_secs()
	}
}

impl Clone for ReadingInfo {
	fn clone(&self) -> Self
	{
		ReadingInfo {
			row_id: self.row_id,
			filename: self.filename.clone(),
			inner_book: self.inner_book,
			chapter: self.chapter,
			line: self.line,
			position: self.position,
			custom_color: self.custom_color,
			strip_empty_lines: self.strip_empty_lines,
		}
	}
}

impl Listable for ReadingInfo {
	fn title(&self) -> &str {
		&self.filename
	}

	fn id(&self) -> usize
	{
		let rowid = self.row_id;
		if rowid < 0 {
			0
		} else {
			rowid as usize
		}
	}
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct PathConfig {
	pub enabled: bool,
	pub path: PathBuf,
}

#[derive(Serialize, Deserialize, Clone)]
#[cfg(feature = "gui")]
pub struct GuiConfiguration {
	pub fonts: Vec<PathConfig>,
	pub font_size: u8,
	pub sidebar_size: u32,
	#[serde(default = "default_locale")]
	pub lang: String,
	pub dictionaries: Vec<PathConfig>,
	pub cache_dict: bool,
	pub strip_empty_lines: bool,
	pub ignore_font_weight: bool,
}

#[cfg(feature = "gui")]
impl Default for GuiConfiguration
{
	fn default() -> Self {
		GuiConfiguration {
			fonts: vec![],
			font_size: 20,
			sidebar_size: 300,
			lang: default_locale(),
			dictionaries: vec![],
			cache_dict: false,
			strip_empty_lines: false,
			ignore_font_weight: false,
		}
	}
}

#[derive(Serialize, Deserialize)]
pub struct Configuration {
	pub render_han: bool,
	pub current: Option<String>,
	pub dark_theme: bool,
	#[cfg(feature = "gui")]
	#[serde(default)]
	pub gui: GuiConfiguration,
	#[serde(skip)]
	config_file: PathBuf,
	#[serde(skip, default = "default_history_db")]
	history_db: Connection,
}

impl Configuration {
	pub fn save(&self) -> Result<()>
	{
		let text = toml::to_string(self)?;
		fs::write(&self.config_file, text)?;
		Ok(())
	}

	fn map(row: &Row) -> rusqlite::Result<ReadingInfo>
	{
		Ok(ReadingInfo {
			row_id: row.get(0)?,
			filename: row.get(1)?,
			inner_book: row.get(2)?,
			chapter: row.get(3)?,
			line: row.get(4)?,
			position: row.get(5)?,
			custom_color: row.get(6)?,
			strip_empty_lines: row.get(7)?,
		})
	}

	fn query(&self, limit: usize, exclude: &Option<String>) -> Result<Vec<ReadingInfo>>
	{
		let mut stmt = self.history_db.prepare("
select row_id,
       filename,
       inner_book,
       chapter,
       line,
       position,
       custom_color,
       strip_empty_lines,
       ts
from history
order by ts desc
")?;
		let iter = stmt.query_map([], Configuration::map)?;
		let mut list = vec![];
		for info in iter {
			let info = info?;
			let path = PathBuf::from_str(&info.filename)?;
			if !path.exists() {
				continue;
			}
			if let Some(exclude) = exclude {
				if *exclude == info.filename {
					continue;
				}
			}
			list.push(info);
			if list.len() >= limit {
				break;
			}
		}
		Ok(list)
	}

	pub fn history(&self) -> Result<Vec<ReadingInfo>>
	{
		Ok(self.query(20, &self.current)?)
	}

	pub fn latest_reading(&self) -> Result<Option<ReadingInfo>>
	{
		Ok(self.query(1, &None)?.pop())
	}

	pub fn reading(&self, filename: &str) -> Result<(bool, ReadingInfo)>
	{
		let mut stmt = self.history_db.prepare("
select row_id,
       filename,
       inner_book,
       chapter,
       line,
       position,
       custom_color,
       strip_empty_lines,
       ts
from history
where filename = ?
")?;
		let mut iter = stmt.query_map([filename], Configuration::map)?;
		if let Some(info) = iter.next() {
			Ok((true, info?))
		} else {
			Ok((false, ReadingInfo::new(filename)))
		}
	}

	pub fn reading_by_id(&self, row_id: i64) -> Result<ReadingInfo>
	{
		let mut stmt = self.history_db.prepare("
select row_id,
       filename,
       inner_book,
       chapter,
       line,
       position,
       custom_color,
       strip_empty_lines,
       ts
from history
where row_id = ?
")?;
		let mut iter = stmt.query_map([row_id], Configuration::map)?;
		if let Some(info) = iter.next() {
			Ok(info?)
		} else {
			panic!("Reading history not exists");
		}
	}

	pub fn save_reading(&self, reading: &ReadingInfo) -> Result<()>
	{
		let ts = ReadingInfo::now();
		if reading.row_id == 0 {
			self.history_db.execute("
insert into history (filename, inner_book, chapter, line, position,
                     custom_color, strip_empty_lines, ts)
values (?, ?, ?, ?, ?, ?, ?, ?)
", (&reading.filename, reading.inner_book, reading.chapter, reading.line,
				reading.position, reading.custom_color, reading.strip_empty_lines,
				ts))?;
		} else {
			self.history_db.execute("
update history
set filename          = ?,
    inner_book        = ?,
    chapter           = ?,
    line              = ?,
    position          = ?,
    custom_color      = ?,
    strip_empty_lines = ?,
    ts                = ?
where row_id = ?
", (&reading.filename, reading.inner_book, reading.chapter, reading.line,
				reading.position, reading.custom_color, reading.strip_empty_lines,
				ts, reading.row_id))?;
		}
		Ok(())
	}
}

#[derive(Clone)]
pub struct Themes {
	bright: Theme,
	dark: Theme,
}

impl Themes {
	pub fn get(&self, dark: bool) -> &Theme
	{
		if dark {
			&self.dark
		} else {
			&self.bright
		}
	}
}

pub enum BookToOpen {
	None,
	Cmd(String),
	Env(String),
}

impl BookToOpen {
	#[inline]
	fn name(&self) -> Option<&str>
	{
		match self {
			BookToOpen::None => None,
			BookToOpen::Cmd(name) => Some(name),
			BookToOpen::Env(name) => Some(name)
		}
	}
}

pub(super) fn load_config(filename: &BookToOpen, config_file: PathBuf, themes_dir: &PathBuf,
	cache_dir: &PathBuf) -> Result<(Configuration, Themes)>
{
	let (configuration, themes) =
		if config_file.as_path().is_file() {
			let string = fs::read_to_string(&config_file)?;
			let mut configuration: Configuration = toml::from_str(&string)?;
			if let Some(filename) = filename.name() {
				configuration.current = file_path(filename);
			}
			if configuration.current.is_none() {
				if let Some(latest_reading) = configuration.latest_reading()? {
					configuration.current = Some(latest_reading.filename);
				}
			}
			let theme_file = themes_dir.join("dark.toml");
			let dark = process_theme_result(load_theme_file(theme_file))?;
			let theme_file = themes_dir.join("bright.toml");
			let bright = process_theme_result(load_theme_file(theme_file))?;
			let themes = Themes { dark, bright };
			configuration.config_file = config_file;
			(configuration, themes)
		} else {
			let themes = create_default_theme_files(themes_dir)?;
			fs::create_dir_all(cache_dir)?;
			let filepath = filename.name()
				.map_or(None, |filename| file_path(filename));

			(Configuration {
				render_han: false,
				current: filepath,
				dark_theme: false,
				#[cfg(feature = "gui")]
				gui: Default::default(),
				config_file,
				history_db: default_history_db(),
			}, themes)
		};
	return Ok((configuration, themes));
}

fn process_theme_result(result: Result<Theme, Error>) -> Result<Theme> {
	match result {
		Ok(theme) => Ok(theme),
		Err(e) => Err(anyhow!(match e {
					Error::Io(e) => e.to_string(),
					Error::Parse(e) => e.to_string(),
				}))
	}
}

fn create_default_theme_files(themes_dir: &PathBuf) -> Result<Themes>
{
	fs::create_dir_all(themes_dir)?;

	let utf8 = Asset::get("dark.toml").unwrap();
	let str = std::str::from_utf8(utf8.data.as_ref())?;
	let dark = process_theme_result(load_toml(str))?;
	let theme_file = themes_dir.join("dark.toml");
	fs::write(theme_file, str)?;

	let utf8 = Asset::get("bright.toml").unwrap();
	let str = std::str::from_utf8(utf8.data.as_ref())?;
	let bright = process_theme_result(load_toml(str))?;
	let theme_file = themes_dir.join("bright.toml");
	fs::write(theme_file, str)?;

	Ok(Themes { dark, bright })
}

fn file_path(filename: &str) -> Option<String> {
	let filepath = PathBuf::from(filename);
	if !filepath.exists() {
		return None;
	}
	if !filepath.is_file() {
		return None;
	}
	if let Ok(filepath) = fs::canonicalize(filepath) {
		let filename = filepath.as_os_str().to_str().unwrap().to_string();
		Some(filename)
	} else {
		None
	}
}

#[inline]
#[cfg(feature = "i18n")]
fn default_locale() -> String
{
	use sys_locale::get_locale;
	get_locale().unwrap_or_else(|| String::from(i18n::DEFAULT_LOCALE))
}

#[inline]
fn chk<T>(result: rusqlite::Result<T>) -> T
{
	match result {
		Ok(result) => result,
		Err(err) => panic!("Failed on history db: {}", err.to_string()),
	}
}


#[inline]
fn default_history_db() -> Connection
{
	#[inline]
	fn open(path: PathBuf) -> Connection
	{
		match Connection::open(&path) {
			Ok(conn) => conn,
			Err(err) => panic!("Failed open history db: {}", err.to_string()),
		}
	}

	// config_dir is validated before deserialize
	let config_dir = config_dir().unwrap();
	let my_dir = config_dir.join(package_name!());
	let history_db = my_dir.join("history.sqlite");
	if !history_db.exists() {
		// init db
		let conn = open(history_db);
		chk(conn.execute("
create table info ( version integer )
			", ()));
		chk(conn.execute("
create table history
(
    row_id            integer primary key,
    filename          varchar,
    inner_book        unsigned big int,
    chapter           unsigned big int,
    line              unsigned big int,
    position          unsigned big int,
    custom_color      unsigned big int,
    strip_empty_lines unsigned big int,
    ts                unsigned big int,
    unique (filename)
)", ()));
		conn
	} else {
		open(history_db)
	}
}