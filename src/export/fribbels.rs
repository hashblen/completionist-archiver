//! Output format based on the format used by [Fribbels HSR Optimizer],
//! devised by [kel-z's HSR-Scanner].
//!
//! [Fribbels HSR Optimizer]: https://github.com/fribbels/hsr-optimizer
//! [kel-z's HSR-Scanner]: https://github.com/kel-z/HSR-Scanner
use std::collections::HashMap;

use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use reliquary::network::GameCommand;
use reliquary::network::gen::command_id;
use reliquary::network::gen::proto::GetBagScRsp::GetBagScRsp;
use reliquary::network::gen::proto::GetQuestDataScRsp::GetQuestDataScRsp;
use reliquary::network::gen::proto::Material::Material;
use reliquary::network::gen::proto::PlayerGetTokenScRsp::PlayerGetTokenScRsp;
use reliquary::network::gen::proto::Quest::Quest;
use reliquary::network::gen::proto::QuestStatus::QuestStatus::{QUEST_CLOSE, QUEST_FINISH};
use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use tracing::{debug, info, instrument, trace, warn};

use crate::export::Exporter;

const BASE_RESOURCE_URL: &str = "https://raw.githubusercontent.com/Dimbreath/StarRailData/master";

#[derive(Serialize, Deserialize, Debug)]
pub struct Export {
    pub source: &'static str,
    pub build: &'static str,
    pub version: u32,
    pub metadata: Metadata,
    achievements: Vec<u32>,
    books: Vec<u32>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Metadata {
    pub uid: Option<u32>,
}

pub struct OptimizerExporter {
    database: Database,
    uid: Option<u32>,
    achievements: Vec<u32>,
    books: Vec<u32>,
}

impl OptimizerExporter {
    pub fn new(database: Database) -> OptimizerExporter {
        OptimizerExporter {
            database,
            uid: None,
            achievements: vec![],
            books: vec![],
        }
    }

    pub fn set_uid(&mut self, uid: u32) {
        self.uid = Some(uid);
    }

    pub fn add_inventory(&mut self, bag: GetBagScRsp) {
        let books: Vec<Book> = bag.material_list.iter()
            .filter_map(|r| export_proto_book(&self.database, r))
            .collect();

        info!(num=books.len(), "found books");
        let mut ids: Vec<u32> = books.iter().map(|book| book.id.clone()).collect();
        self.books.append(&mut ids);
    }

    pub fn add_achievements(&mut self, quest: GetQuestDataScRsp ) {
        let achievements: Vec<Achievement> = quest.quest_list.iter()
            .filter_map(|r| export_proto_achievement(&self.database, r))
            .collect();

        info!(num=achievements.len(), "found achievements");
        let mut ids: Vec<u32> = achievements.iter().map(|achievement| achievement.id.clone()).collect();
        self.achievements.append(&mut ids);
    }
}

impl Exporter for OptimizerExporter {
    type Export = Export;

    fn read_command(&mut self, command: GameCommand) {
        match command.command_id {
            command_id::PlayerGetTokenScRsp => {
                debug!("detected uid");
                let cmd = command.parse_proto::<PlayerGetTokenScRsp>();
                match cmd {
                    Ok(cmd) => {
                        self.set_uid(cmd.uid)
                    }
                    Err(error) => {
                        warn!(%error, "could not parse token command");
                    }
                }
            }
            command_id::GetBagScRsp => {
                debug!("detected inventory packet");
                let cmd = command.parse_proto::<GetBagScRsp>();
                match cmd {
                    Ok(cmd) => {
                        self.add_inventory(cmd)
                    }
                    Err(error) => {
                        warn!(%error, "could not parse inventory data command");
                    }
                }
            }
            command_id::GetQuestDataScRsp => {
                debug!("detected quest packet");
                let cmd = command.parse_proto::<GetQuestDataScRsp>();
                match cmd {
                    Ok(cmd) => {
                        self.add_achievements(cmd)
                    }
                    Err(error) => {
                        warn!(%error, "could not parse quest data command");
                    }
                }
            }
            _ => {
                trace!(command_id=command.command_id, tag=command.get_command_name(), "ignored");
            }
        }
    }

    fn is_finished(&self) -> bool {
        self.uid.is_some()
            && !self.achievements.is_empty()
            && !self.books.is_empty()
    }

    #[instrument(skip_all)]
    fn export(self) -> Self::Export {
        info!("exporting collected data");

        if self.uid.is_none() {
            warn!("uid was not recorded");
        }

        if self.achievements.is_empty() {
            warn!("relics were not recorded");
        }

        if self.books.is_empty() {
            warn!("relics were not recorded");
        }

        Export {
            source: "completionist_archiver",
            build: env!("CARGO_PKG_VERSION"),
            version: 3,
            metadata: Metadata {
                uid: self.uid,
            },
            achievements: self.achievements,
            books: self.books,
        }
    }
}

pub struct Database {
    achievement_list: Vec<u32>,
    book_list: Vec<u32>,
    // text_map: TextMap,
    keys: HashMap<u32, Vec<u8>>,
}

impl Database {
    #[instrument(name = "config_map")]
    pub fn new_from_online() -> Self {
        info!("initializing database from online sources, this might take a while...");
        Database {
            achievement_list: Self::load_online_achievement_list(),
            book_list: Self::load_online_book_list(),
            // text_map: Self::load_online_text_map(),
            keys: Self::load_online_keys(),
        }
    }
    // TODO: new_from_source

    fn load_online_achievement_list() -> Vec<u32> {
        let json_object = Self::get_json(format!("{BASE_RESOURCE_URL}/ExcelOutput/AchievementData.json"));
        let mut achievement_list = vec![];
        for (_key, value) in json_object.as_object().unwrap() {
            let achievement_id: u32 = value["AchievementID"].as_u64().unwrap() as u32;
            achievement_list.push(achievement_id)
        }
        achievement_list
    }
    fn load_online_book_list() -> Vec<u32> {
        let json_object = Self::get_json(format!("{BASE_RESOURCE_URL}/ExcelOutput/LocalbookConfig.json"));
        let mut book_list = vec![];
        for (_key, value) in json_object.as_object().unwrap() {
            let book_id: u32 = value["BookID"].as_u64().unwrap() as u32;
            book_list.push(book_id)
        }
        book_list
    }
    /*fn load_online_text_map() -> TextMap {
        Self::get(format!("{BASE_RESOURCE_URL}/TextMap/TextMapEN.json"))
    }*/

    fn load_online_keys() -> HashMap<u32, Vec<u8>> {
        let keys: HashMap<u32, String> = Self::get("https://raw.githubusercontent.com/tamilpp25/Iridium-SR/main/data/Keys.json".to_string());
        let mut keys_bytes = HashMap::new();

        for (k, v) in keys {
            keys_bytes.insert(k, BASE64_STANDARD.decode(v).unwrap());
        }

        keys_bytes
    }

    fn get<T: DeserializeOwned>(url: String) -> T {
        debug!(url, "requesting from resource");
        ureq::get(&url)
            .call()
            .unwrap()
            .into_json()
            .unwrap()
    }

    fn get_json(url: String) -> serde_json::Value {
        debug!(url, "requesting from resource");
        ureq::get(&url)
            .call()
            .unwrap()
            .into_json()
            .unwrap()
    }

    pub fn keys(&self) -> &HashMap<u32, Vec<u8>> {
        &self.keys
    }
}

#[tracing::instrument(name = "achievement", skip_all, fields(id = proto.id))]
fn export_proto_achievement(db: &Database, proto: &Quest) -> Option<Achievement> {
    if (proto.status.unwrap() == QUEST_CLOSE || proto.status.unwrap() == QUEST_FINISH) && db.achievement_list.contains(&proto.id) {
        Some(Achievement {
            id: proto.id,
        })
    }
    else {
        None
    }
}

#[tracing::instrument(name = "book", skip_all, fields(id = proto.tid))]
fn export_proto_book(db: &Database, proto: &Material) -> Option<Book> {
    if db.book_list.contains(&proto.tid) {
        Some(Book {
            id: proto.tid,
        })
    }
    else {
        None
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Achievement {
    pub id: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Book {
    pub id: u32,
}