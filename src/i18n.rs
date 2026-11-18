use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Language {
    #[default]
    English,
    Slovak,
    Czech,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::English, Language::Slovak, Language::Czech];

    /// Each language is named in itself, so it can be found whatever language is active.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Slovak => "Slovenčina",
            Language::Czech => "Čeština",
        }
    }
}

/// The interface language is global: every screen reads it, and only the settings change it.
static ACTIVE: AtomicU8 = AtomicU8::new(0);

pub fn set_language(language: Language) {
    ACTIVE.store(language as u8, Ordering::Relaxed);
}

pub fn language() -> Language {
    Language::ALL[ACTIVE.load(Ordering::Relaxed) as usize % Language::ALL.len()]
}

fn translations() -> &'static HashMap<&'static str, (&'static str, &'static str)> {
    static TABLE: OnceLock<HashMap<&'static str, (&'static str, &'static str)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        TEXTS
            .iter()
            .map(|&(english, slovak, czech)| (english, (slovak, czech)))
            .collect()
    })
}

fn lookup(english: &str) -> Option<&'static str> {
    let (slovak, czech) = translations().get(english)?;
    match language() {
        Language::English => None,
        Language::Slovak => Some(slovak),
        Language::Czech => Some(czech),
    }
}

/// The interface text in the active language; texts without a translation stay in English.
pub fn tr(english: &'static str) -> &'static str {
    lookup(english).unwrap_or(english)
}

/// Like `tr`, for texts that are not compile-time constants (saved outcomes, enum names).
pub fn tr_owned(english: &str) -> String {
    lookup(english).map_or_else(|| english.to_string(), str::to_string)
}

/// Translates a template, then fills its `{}` placeholders in order.
pub fn tr_fmt(template: &'static str, values: &[&dyn Display]) -> String {
    let mut parts = tr(template).split("{}");
    let mut filled = parts.next().unwrap_or_default().to_string();
    for (part, value) in parts.zip(
        values
            .iter()
            .map(|v| v.to_string())
            .chain(std::iter::repeat_with(String::new)),
    ) {
        filled.push_str(&value);
        filled.push_str(part);
    }
    filled
}

/// English, Slovak, Czech. Placeholders `{}` must appear the same number of times in every column.
const TEXTS: &[(&str, &str, &str)] = &[
    // Sidebar
    ("BASE", "ZÁKLADŇA", "ZÁKLADNA"),
    ("INFANTRY", "PECHOTA", "PĚCHOTA"),
    ("VEHICLES", "VOZIDLÁ", "VOZIDLA"),
    ("COALITION", "KOALÍCIA", "KOALICE"),
    ("SYNDICATE", "SYNDIKÁT", "SYNDIKÁT"),
    ("NETWORK", "SIEŤ", "SÍŤ"),
    ("Network battle", "Sieťová bitka", "Síťová bitva"),
    (
        "Two players on two computers, one against one. The host plays the Coalition with the battle editor's settings; the other player joins as the Syndicate. Both need the same version of the game, and the host's port 47300 must be reachable.",
        "Dvaja hráči na dvoch počítačoch, jeden proti jednému. Hostiteľ hrá za Koalíciu s nastaveniami editora bitiek, druhý hráč sa pripojí za Syndikát. Obaja potrebujú rovnakú verziu hry a port 47300 hostiteľa musí byť dostupný.",
        "Dva hráči na dvou počítačích, jeden proti jednomu. Hostitel hraje za Koalici s nastavením editoru bitev, druhý hráč se připojí za Syndikát. Oba potřebují stejnou verzi hry a port 47300 hostitele musí být dostupný.",
    ),
    ("HOST A BATTLE", "HOSTIŤ BITKU", "HOSTOVAT BITVU"),
    (
        "Host address (type it)",
        "Adresa hostiteľa (napíšte ju)",
        "Adresa hostitele (napište ji)",
    ),
    ("JOIN", "PRIPOJIŤ SA", "PŘIPOJIT SE"),
    ("CANCEL", "ZRUŠIŤ", "ZRUŠIT"),
    (
        "Cannot host: {}",
        "Nedá sa hostiť: {}",
        "Nelze hostovat: {}",
    ),
    (
        "Waiting for the other player at {} (port {})",
        "Čakám na druhého hráča na {} (port {})",
        "Čekám na druhého hráče na {} (port {})",
    ),
    ("Connecting...", "Pripájam sa...", "Připojuji se..."),
    (
        "Connected, waiting for the host",
        "Pripojené, čakám na hostiteľa",
        "Připojeno, čekám na hostitele",
    ),
    (
        "this computer's address",
        "adresa tohto počítača",
        "adresa tohoto počítače",
    ),
    (
        "Connection failed: {}",
        "Pripojenie zlyhalo: {}",
        "Připojení selhalo: {}",
    ),
    (
        "The host runs version {}; both players need the same version",
        "Hostiteľ má verziu {}; obaja hráči potrebujú rovnakú verziu",
        "Hostitel má verzi {}; oba hráči potřebují stejnou verzi",
    ),
    (
        "The host closed the connection",
        "Hostiteľ ukončil spojenie",
        "Hostitel ukončil spojení",
    ),
    (
        "Not available in a network battle",
        "V sieťovej bitke nie je dostupné",
        "V síťové bitvě není dostupné",
    ),
    (
        "The other player has left the battle",
        "Druhý hráč opustil bitku",
        "Druhý hráč opustil bitvu",
    ),
    (
        "Out of step with the other player at tick {}",
        "Hra sa rozišla s druhým hráčom v kroku {}",
        "Hra se rozešla s druhým hráčem v kroku {}",
    ),
    ("POWER {} / {}", "ENERGIA {} / {}", "ENERGIE {} / {}"),
    ("REPAIR  {}", "OPRAVA  {}", "OPRAVA  {}"),
    ("SELL  {}", "PREDAJ  {}", "PRODEJ  {}"),
    ("READY", "HOTOVO", "HOTOVO"),
    ("LOCKED", "ZAMKNUTÉ", "ZAMČENO"),
    ("N/A", "N/D", "N/D"),
    ("QUEUE", "FRONT", "FRONTA"),
    ("No selection", "Nič nie je vybrané", "Nic není vybráno"),
    (
        "Select units with a click or a drag box.",
        "Jednotky vyberiete kliknutím alebo ťahaním rámčeka.",
        "Jednotky vyberete kliknutím nebo tažením rámečku.",
    ),
    ("{} units", "{} jednotiek", "{} jednotek"),
    ("1 unit", "1 jednotka", "1 jednotka"),
    ("Structure", "Budova", "Budova"),
    ("idle", "nečinný", "nečinný"),
    (
        "heading to crystals",
        "ide ku kryštálom",
        "jede ke krystalům",
    ),
    ("mining", "ťaží", "těží"),
    ("returning", "vracia sa", "vrací se"),
    ("unloading", "vykladá", "vykládá"),
    ("+{} power", "+{} energie", "+{} energie"),
    ("{} power", "{} energie", "{} energie"),
    ("Requires {}", "Vyžaduje {}", "Vyžaduje {}"),
    (
        "{}\nCargo {} / {}  ({})",
        "{}\nNáklad {} / {}  ({})",
        "{}\nNáklad {} / {}  ({})",
    ),
    (
        "{}\n{}s build, {}{}",
        "{}\nvýroba {} s, {}{}",
        "{}\nvýroba {} s, {}{}",
    ),
    (
        "Group {} - {} units",
        "Skupina {} – {} jednotiek",
        "Skupina {} – {} jednotek",
    ),
    ("OBJECTIVE: {}", "CIEĽ: {}", "CÍL: {}"),
    ("{}  controls", "{}  ovládanie", "{}  ovládání"),
    ("CONTROLS", "OVLÁDANIE", "OVLÁDÁNÍ"),
    // Controls overview
    ("Select", "Výber", "Výběr"),
    (
        "left click / drag a box (Shift adds)",
        "ľavý klik / ťahanie rámčeka (Shift pridáva)",
        "levý klik / tažení rámečku (Shift přidává)",
    ),
    ("Orders", "Rozkazy", "Rozkazy"),
    (
        "right click: move, attack, harvest",
        "pravý klik: presun, útok, ťažba",
        "pravý klik: přesun, útok, těžba",
    ),
    ("Attack-move", "Útočný presun", "Útočný přesun"),
    (
        "Ctrl + right click",
        "Ctrl + pravý klik",
        "Ctrl + pravý klik",
    ),
    ("Patrol", "Hliadka", "Hlídka"),
    ("Alt + right click", "Alt + pravý klik", "Alt + pravý klik"),
    ("Queue orders", "Rozkazy za sebou", "Rozkazy za sebou"),
    (
        "Shift + right click",
        "Shift + pravý klik",
        "Shift + pravý klik",
    ),
    ("Rally point", "Zhromaždisko", "Shromaždiště"),
    (
        "select barracks/factory, right click",
        "vyberte kasárne/továreň, pravý klik",
        "vyberte kasárna/továrnu, pravý klik",
    ),
    ("Repair / sell", "Oprava / predaj", "Oprava / prodej"),
    (
        "{} / {} (or the buttons), then click a structure",
        "{} / {} (alebo tlačidlá), potom kliknite na budovu",
        "{} / {} (nebo tlačítka), pak klikněte na budovu",
    ),
    ("Scroll", "Posun", "Posun"),
    (
        "{}{}{}{}, arrows, screen edge, middle-drag",
        "{}{}{}{}, šípky, okraj obrazovky, stredné tlačidlo",
        "{}{}{}{}, šipky, okraj obrazovky, prostřední tlačítko",
    ),
    ("Fast scroll", "Rýchly posun", "Rychlý posun"),
    ("hold Shift", "podržte Shift", "podržte Shift"),
    ("Minimap", "Minimapa", "Minimapa"),
    (
        "click or drag to jump",
        "kliknutím alebo ťahaním skočíte",
        "kliknutím nebo tažením skočíte",
    ),
    (
        "Zoom / rotate",
        "Priblíženie / otáčanie",
        "Přiblížení / otáčení",
    ),
    (
        "mouse wheel / {}, {} ({} resets)",
        "koliesko myši / {}, {} ({} vráti)",
        "kolečko myši / {}, {} ({} vrátí)",
    ),
    ("Home", "Domov", "Domů"),
    ("{} or Home", "{} alebo Home", "{} nebo Home"),
    (
        "Stop / deselect",
        "Stoj / zrušiť výber",
        "Stůj / zrušit výběr",
    ),
    ("Groups", "Skupiny", "Skupiny"),
    (
        "Ctrl+1-9 save, 1-9 select, twice = jump",
        "Ctrl+1-9 uloží, 1-9 vyberie, dvakrát = skok",
        "Ctrl+1-9 uloží, 1-9 vybere, dvakrát = skok",
    ),
    ("Add a group", "Pridať skupinu", "Přidat skupinu"),
    ("Graphics quality", "Kvalita grafiky", "Kvalita grafiky"),
    (
        "{}: Low / Medium / High",
        "{}: nízka / stredná / vysoká",
        "{}: nízká / střední / vysoká",
    ),
    ("Performance", "Výkon", "Výkon"),
    (
        "{} overlay: FPS, CPU, GPU",
        "{} prehľad: FPS, CPU, GPU",
        "{} přehled: FPS, CPU, GPU",
    ),
    ("Save / load", "Uložiť / načítať", "Uložit / načíst"),
    (
        "{} quick save, {} quick load",
        "{} rýchle uloženie, {} rýchle načítanie",
        "{} rychlé uložení, {} rychlé načtení",
    ),
    ("Menu & settings", "Menu a nastavenia", "Menu a nastavení"),
    ("F10 or Esc", "F10 alebo Esc", "F10 nebo Esc"),
    ("Build menu", "Stavebné menu", "Stavební menu"),
    (
        "left click builds (Shift = 5x), right click cancels",
        "ľavý klik stavia (Shift = 5x), pravý ruší",
        "levý klik staví (Shift = 5x), pravý ruší",
    ),
    ("Queue bar", "Lišta frontu", "Lišta fronty"),
    (
        "left click places a ready structure, right click cancels",
        "ľavý klik umiestni hotovú budovu, pravý ruší",
        "levý klik umístí hotovou budovu, pravý ruší",
    ),
    // Repair and sell
    (
        "Repair: pick a structure",
        "Oprava: vyberte budovu",
        "Oprava: vyberte budovu",
    ),
    (
        "Sell: pick a structure",
        "Predaj: vyberte budovu",
        "Prodej: vyberte budovu",
    ),
    ("Undamaged", "Nepoškodená", "Nepoškozená"),
    ("Repair  ${}", "Oprava  ${}", "Oprava  ${}"),
    ("Sell  +${}", "Predaj  +${}", "Prodej  +${}"),
    ("Cannot be sold", "Nedá sa predať", "Nelze prodat"),
    // Notifications
    (
        "Our base is under attack!",
        "Naša základňa je pod útokom!",
        "Naše základna je pod útokem!",
    ),
    (
        "Low power - production slowed",
        "Málo energie – výroba sa spomalila",
        "Málo energie – výroba se zpomalila",
    ),
    (
        "Construction complete",
        "Stavba dokončená",
        "Stavba dokončena",
    ),
    ("{} ready", "{} pripravený", "{} připraven"),
    (
        "Not available in this mission",
        "V tejto misii nie je k dispozícii",
        "V této misi není k dispozici",
    ),
    (
        "Production queue is full",
        "Výrobný front je plný",
        "Výrobní fronta je plná",
    ),
    (
        "Insufficient funds",
        "Nedostatok kreditov",
        "Nedostatek kreditů",
    ),
    (
        "{} captured by the enemy",
        "{} obsadil nepriateľ",
        "{} obsadil nepřítel",
    ),
    ("{} captured", "{} obsadená", "{} obsazena"),
    ("{} repaired", "{} opravená", "{} opravena"),
    (
        "{} promoted to {}",
        "{} povýšený na: {}",
        "{} povýšen na: {}",
    ),
    ("Recruit", "Nováčik", "Nováček"),
    ("Veteran", "Veterán", "Veterán"),
    ("Elite", "Elita", "Elita"),
    ("Game saved - {}", "Hra uložená – {}", "Hra uložena – {}"),
    ("Saving failed", "Uloženie zlyhalo", "Uložení selhalo"),
    ("Game loaded", "Hra načítaná", "Hra načtena"),
    (
        "No saved game in that slot",
        "Na tejto pozícii nie je uložená hra",
        "Na této pozici není uložená hra",
    ),
    (
        "Saved game is damaged",
        "Uložená hra je poškodená",
        "Uložená hra je poškozená",
    ),
    (
        "Saved game is from an incompatible version",
        "Uložená hra je z nekompatibilnej verzie",
        "Uložená hra je z nekompatibilní verze",
    ),
    (
        "The replay could not be found",
        "Záznam sa nenašiel",
        "Záznam nebyl nalezen",
    ),
    (
        "Graphics quality: {}",
        "Kvalita grafiky: {}",
        "Kvalita grafiky: {}",
    ),
    // Saves
    ("Quick save", "Rýchle uloženie", "Rychlé uložení"),
    // Key bindings
    ("KEYS", "KLÁVESY", "KLÁVESY"),
    ("Scroll up", "Posun hore", "Posun nahoru"),
    ("Scroll down", "Posun dole", "Posun dolů"),
    ("Scroll left", "Posun vľavo", "Posun vlevo"),
    ("Scroll right", "Posun vpravo", "Posun vpravo"),
    ("Rotate left", "Otočiť doľava", "Otočit doleva"),
    ("Rotate right", "Otočiť doprava", "Otočit doprava"),
    ("Reset rotation", "Zrušiť otočenie", "Zrušit otočení"),
    ("Jump to base", "Skok na základňu", "Skok na základnu"),
    ("Stop", "Zastaviť", "Zastavit"),
    ("Repair mode", "Režim opravy", "Režim opravy"),
    ("Sell mode", "Režim predaja", "Režim prodeje"),
    ("Quick load", "Rýchle načítanie", "Rychlé načtení"),
    ("Controls overview", "Prehľad ovládania", "Přehled ovládání"),
    ("Performance overlay", "Prehľad výkonu", "Přehled výkonu"),
    ("press a key", "stlačte kláves", "stiskněte klávesu"),
    ("Reset to defaults", "Obnoviť predvolené", "Obnovit výchozí"),
    (
        "Click an action, then press its new key. A key already in use swaps places. Esc cancels.",
        "Kliknite na akciu a stlačte nový kláves. Obsadený kláves sa vymení. Esc zruší.",
        "Klikněte na akci a stiskněte novou klávesu. Obsazená klávesa se prohodí. Esc zruší.",
    ),
    ("Space", "Medzerník", "Mezerník"),
    ("Slot {}", "Pozícia {}", "Pozice {}"),
    ("just now", "práve teraz", "právě teď"),
    ("{} min ago", "pred {} min", "před {} min"),
    ("{} h ago", "pred {} h", "před {} h"),
    ("{} days ago", "pred {} dňami", "před {} dny"),
    (
        "{}  -  ${}, {} units",
        "{}  -  ${}, {} jednotiek",
        "{}  -  ${}, {} jednotek",
    ),
    ("empty", "prázdne", "prázdné"),
    ("Save", "Uložiť", "Uložit"),
    ("Load", "Načítať", "Načíst"),
    // Objectives
    (
        "Destroy every enemy unit and structure",
        "Zničte všetky nepriateľské jednotky a budovy",
        "Zničte všechny nepřátelské jednotky a budovy",
    ),
    (
        "Destroy all enemy Command Centers",
        "Zničte všetky nepriateľské veliteľstvá",
        "Zničte všechna nepřátelská velitelství",
    ),
    (
        "Capture an enemy Command Center with an engineer",
        "Obsaďte nepriateľské veliteľstvo ženistom",
        "Obsaďte nepřátelské velitelství ženistou",
    ),
    (
        "Hold out for {} minutes",
        "Vydržte {} minút",
        "Vydržte {} minut",
    ),
    ("Stockpile ${}", "Nazbierajte ${}", "Nashromážděte ${}"),
    ("  -  {} left", "  -  zostáva {}", "  -  zbývá {}"),
    // End of the match
    ("VICTORY", "VÍŤAZSTVO", "VÍTĚZSTVÍ"),
    ("DEFEAT", "PORÁŽKA", "PORÁŽKA"),
    ("WATCH REPLAY", "POZRIEŤ ZÁZNAM", "PŘEHRÁT ZÁZNAM"),
    ("MAIN MENU", "HLAVNÉ MENU", "HLAVNÍ MENU"),
    (
        "F10 opens the menu to restart the mission.",
        "F10 otvorí menu, kde môžete misiu zopakovať.",
        "F10 otevře menu, kde můžete misi opakovat.",
    ),
    ("MATCH TIME  {}", "ČAS BITKY  {}", "ČAS BITVY  {}"),
    ("You", "Vy", "Vy"),
    ("Enemies", "Nepriatelia", "Nepřátelé"),
    ("Units trained", "Vycvičené jednotky", "Vycvičené jednotky"),
    ("Units lost", "Stratené jednotky", "Ztracené jednotky"),
    ("Structures built", "Postavené budovy", "Postavené budovy"),
    ("Structures lost", "Stratené budovy", "Ztracené budovy"),
    (
        "Enemies destroyed",
        "Zničení nepriatelia",
        "Zničení nepřátelé",
    ),
    (
        "Damage dealt",
        "Spôsobené poškodenie",
        "Způsobené poškození",
    ),
    ("Credits harvested", "Vyťažené kredity", "Vytěžené kredity"),
    ("Victory", "Víťazstvo", "Vítězství"),
    ("Defeat", "Porážka", "Porážka"),
    ("Unfinished", "Nedokončená", "Nedokončená"),
    // Tutorial
    ("TUTORIAL", "VÝUKA", "VÝUKA"),
    ("TUTORIAL  {} / {}", "VÝUKA  {} / {}", "VÝUKA  {} / {}"),
    ("hide  ×", "skryť  ×", "skrýt  ×"),
    (
        "Select your troops: drag a box around them with the left mouse button.",
        "Vyberte svoje jednotky: ťahajte okolo nich rámček ľavým tlačidlom myši.",
        "Vyberte své jednotky: táhněte kolem nich rámeček levým tlačítkem myši.",
    ),
    (
        "Right-click the ground to send the selected troops there.",
        "Pravým klikom na zem pošlete vybrané jednotky na to miesto.",
        "Pravým kliknutím na zem pošlete vybrané jednotky na to místo.",
    ),
    (
        "Open the BASE tab, click Power Plant, and place it next to your Command Center when it is ready.",
        "Otvorte záložku ZÁKLADŇA, kliknite na Elektráreň a keď bude hotová, postavte ju vedľa veliteľstva.",
        "Otevřete záložku ZÁKLADNA, klikněte na Elektrárnu a až bude hotová, postavte ji vedle velitelství.",
    ),
    (
        "Build a Refinery near the crystals. It comes with a harvester that earns credits.",
        "Postavte Rafinériu pri kryštáloch. Dostanete k nej zberač, ktorý zarába kredity.",
        "Postavte Rafinérii u krystalů. Dostanete k ní sběrač, který vydělává kredity.",
    ),
    (
        "Build Barracks to train infantry.",
        "Postavte Kasárne na výcvik pechoty.",
        "Postavte Kasárna pro výcvik pěchoty.",
    ),
    (
        "Open the INFANTRY tab and train a Rifleman.",
        "Otvorte záložku PECHOTA a vycvičte Strelca.",
        "Otevřete záložku PĚCHOTA a vycvičte Střelce.",
    ),
    (
        "Select your troops and Ctrl + right-click across the river: they fight anything on the way.",
        "Vyberte jednotky a Ctrl + pravým klikom ich pošlite cez rieku: cestou bojujú so všetkým.",
        "Vyberte jednotky a Ctrl + pravým kliknutím je pošlete přes řeku: cestou bojují se vším.",
    ),
    (
        "Destroy the Syndicate outpost in the north-east. Press {} any time for all controls.",
        "Zničte základňu Syndikátu na severovýchode. {} kedykoľvek ukáže celé ovládanie.",
        "Zničte základnu Syndikátu na severovýchodě. {} kdykoli ukáže celé ovládání.",
    ),
    // Replays
    ("paused", "pauza", "pauza"),
    (
        "REPLAY  {}  ·  {} / {}  ·  {}  ·  {}",
        "ZÁZNAM  {}  ·  {} / {}  ·  {}  ·  {}",
        "ZÁZNAM  {}  ·  {} / {}  ·  {}  ·  {}",
    ),
    (
        "Space pause  ·  + / - speed  ·  Left / Right 10 s  ·  Home restart  ·  Esc menu",
        "Medzerník pauza  ·  + / - rýchlosť  ·  Vľavo / Vpravo 10 s  ·  Home od začiatku  ·  Esc menu",
        "Mezerník pauza  ·  + / - rychlost  ·  Vlevo / Vpravo 10 s  ·  Home od začátku  ·  Esc menu",
    ),
    // In-game menu
    ("GAME", "HRA", "HRA"),
    ("GRAPHICS", "GRAFIKA", "GRAFIKA"),
    ("GAMEPLAY", "HRATEĽNOSŤ", "HRATELNOST"),
    ("AUDIO", "ZVUK", "ZVUK"),
    ("SAVE / LOAD", "ULOŽIŤ / NAČÍTAŤ", "ULOŽIT / NAČÍST"),
    (
        "paused  -  Esc / F10 to resume",
        "pauza  -  Esc / F10 pokračuje",
        "pauza  -  Esc / F10 pokračuje",
    ),
    (
        "Battle is paused. Graphics and gameplay options are saved automatically.\n{} quick save, {} quick load, {} performance overlay.",
        "Bitka je pozastavená. Nastavenia grafiky a hry sa ukladajú automaticky.\n{} rýchle uloženie, {} rýchle načítanie, {} prehľad výkonu.",
        "Bitva je pozastavena. Nastavení grafiky a hry se ukládá automaticky.\n{} rychlé uložení, {} rychlé načtení, {} přehled výkonu.",
    ),
    ("Resume", "Pokračovať", "Pokračovat"),
    ("Restart mission", "Reštartovať misiu", "Restartovat misi"),
    (
        "Back to main menu",
        "Späť do hlavného menu",
        "Zpět do hlavního menu",
    ),
    ("Quit to desktop", "Ukončiť hru", "Ukončit hru"),
    (
        "Left click: next value, right click: previous.",
        "Ľavý klik: ďalšia hodnota, pravý klik: predchádzajúca.",
        "Levý klik: další hodnota, pravý klik: předchozí.",
    ),
    ("Language", "Jazyk", "Jazyk"),
    ("Quality preset", "Kvalita grafiky", "Kvalita grafiky"),
    ("Shadows", "Tiene", "Stíny"),
    (
        "Ambient occlusion",
        "Okolité zatienenie",
        "Okolní zastínění",
    ),
    ("Glow (bloom)", "Žiara (bloom)", "Záře (bloom)"),
    (
        "Anti-aliasing (SMAA)",
        "Vyhladzovanie (SMAA)",
        "Vyhlazování (SMAA)",
    ),
    ("Grass and flowers", "Tráva a kvety", "Tráva a květiny"),
    (
        "Wind in vegetation",
        "Vietor vo vegetácii",
        "Vítr ve vegetaci",
    ),
    ("Frame rate limit", "Limit snímok", "Limit snímků"),
    ("Fullscreen", "Celá obrazovka", "Celá obrazovka"),
    ("Game speed", "Rýchlosť hry", "Rychlost hry"),
    ("Edge scrolling", "Posun okrajom", "Posun okrajem"),
    ("Scroll speed", "Rýchlosť posunu", "Rychlost posunu"),
    ("Damage numbers", "Čísla poškodenia", "Čísla poškození"),
    ("Health bars", "Ukazovatele zdravia", "Ukazatele zdraví"),
    (
        "Performance overlay (F3)",
        "Prehľad výkonu (F3)",
        "Přehled výkonu (F3)",
    ),
    ("Master volume", "Celková hlasitosť", "Celková hlasitost"),
    ("Music volume", "Hlasitosť hudby", "Hlasitost hudby"),
    ("Effects volume", "Hlasitosť efektov", "Hlasitost efektů"),
    ("On", "Zapnuté", "Zapnuto"),
    ("Off", "Vypnuté", "Vypnuto"),
    ("Unlimited", "Neobmedzené", "Neomezeno"),
    ("Always", "Vždy", "Vždy"),
    (
        "Damaged / selected",
        "Poškodené / vybrané",
        "Poškozené / vybrané",
    ),
    ("Selected only", "Len vybrané", "Jen vybrané"),
    ("Low", "Nízka", "Nízká"),
    ("Medium", "Stredná", "Střední"),
    ("High", "Vysoká", "Vysoká"),
    ("Custom", "Vlastná", "Vlastní"),
    ("Near", "Blízko", "Blízko"),
    ("Far", "Ďaleko", "Daleko"),
    ("Slow", "Pomalá", "Pomalá"),
    ("Normal", "Normálna", "Normální"),
    ("Fast", "Rýchla", "Rychlá"),
    ("Easy", "Ľahká", "Lehká"),
    ("Hard", "Ťažká", "Těžká"),
    // Main menu
    (
        "Coalition command",
        "Veliteľstvo Koalície",
        "Velitelství Koalice",
    ),
    ("Continue  ({})", "Pokračovať  ({})", "Pokračovat  ({})"),
    ("CAMPAIGN", "KAMPAŇ", "KAMPAŇ"),
    ("CUSTOM LEVELS", "VLASTNÉ MAPY", "VLASTNÍ MAPY"),
    (
        "No custom levels yet - make one in the battle editor.",
        "Zatiaľ žiadne vlastné mapy – vytvorte si ju v editore bitky.",
        "Zatím žádné vlastní mapy – vytvořte si ji v editoru bitvy.",
    ),
    ("QUICK BATTLE", "RÝCHLA BITKA", "RYCHLÁ BITVA"),
    ("REPLAYS", "ZÁZNAMY", "ZÁZNAMY"),
    ("Battle editor", "Editor bitky", "Editor bitvy"),
    ("START MISSION", "SPUSTIŤ MISIU", "SPUSTIT MISI"),
    ("START BATTLE", "SPUSTIŤ BITKU", "SPUSTIT BITVU"),
    (
        "SAVE AS CUSTOM LEVEL",
        "ULOŽIŤ AKO VLASTNÚ MAPU",
        "ULOŽIT JAKO VLASTNÍ MAPU",
    ),
    ("Objective: {}", "Cieľ: {}", "Cíl: {}"),
    (
        "Your base: {}  -  ${}",
        "Vaša základňa: {}  -  ${}",
        "Vaše základna: {}  -  ${}",
    ),
    ("every unit", "všetky jednotky", "všechny jednotky"),
    ("Available: {}", "K dispozícii: {}", "K dispozici: {}"),
    (
        "Saved as {} - it now appears under Custom levels.",
        "Uložené ako {} – nájdete ju medzi vlastnými mapami.",
        "Uloženo jako {} – najdete ji mezi vlastními mapami.",
    ),
    (
        "Could not save the level: {}",
        "Mapu sa nepodarilo uložiť: {}",
        "Mapu se nepodařilo uložit: {}",
    ),
    (
        "Could not open the replay: {}",
        "Záznam sa nepodarilo otvoriť: {}",
        "Záznam se nepodařilo otevřít: {}",
    ),
    (
        "Watch the whole battle again without the fog of war. Space pauses, + and - change the speed, Esc returns here.",
        "Pozrite si celú bitku znova bez hmly vojny. Medzerník pozastaví, + a - menia rýchlosť, Esc vráti sem.",
        "Podívejte se na celou bitvu znovu bez mlhy války. Mezerník pozastaví, + a - mění rychlost, Esc vrátí sem.",
    ),
    (
        "Enemy commanders",
        "Nepriateľskí velitelia",
        "Nepřátelští velitelé",
    ),
    ("Difficulty", "Obtiažnosť", "Obtížnost"),
    (
        "Starting credits",
        "Počiatočné kredity",
        "Počáteční kredity",
    ),
    ("Objective", "Cieľ", "Cíl"),
    ("Available units", "Dostupné jednotky", "Dostupné jednotky"),
    ("Map variant", "Variant mapy", "Varianta mapy"),
    ("Your base", "Vaša základňa", "Vaše základna"),
    ("Fog of war", "Hmla vojny", "Mlha války"),
    ("Opponents", "Protivníci", "Protivníci"),
    ("Landscape", "Krajina", "Krajina"),
    ("Weather", "Počasie", "Počasí"),
    ("Destroy everything", "Zničiť všetko", "Zničit vše"),
    (
        "Destroy Command Centers",
        "Zničiť veliteľstvá",
        "Zničit velitelství",
    ),
    ("Survive 10 minutes", "Prežiť 10 minút", "Přežít 10 minut"),
    (
        "Stockpile credits",
        "Nazbierať kredity",
        "Nashromáždit kredity",
    ),
    ("{} of {}", "{} z {}", "{} z {}"),
    ("Free-for-all", "Každý proti každému", "Každý proti každému"),
    ("Allied", "Spojenci", "Spojenci"),
    ("River", "Rieka", "Řeka"),
    ("Lake", "Jazero", "Jezero"),
    ("Plains", "Pláne", "Pláně"),
    ("Clear", "Jasno", "Jasno"),
    ("Rain", "Dážď", "Déšť"),
    (
        "Basic (no heavy units)",
        "Základná (bez ťažkých jednotiek)",
        "Základní (bez těžkých jednotek)",
    ),
    (
        "Standard (no Bulwark)",
        "Štandardná (bez Bašty)",
        "Standardní (bez Bašty)",
    ),
    ("Full arsenal", "Plný arzenál", "Plný arzenál"),
    ("South-west", "Juhozápad", "Jihozápad"),
    ("North-east", "Severovýchod", "Severovýchod"),
    ("North-west", "Severozápad", "Severozápad"),
    ("South-east", "Juhovýchod", "Jihovýchod"),
    // Factions
    ("Coalition", "Koalícia", "Koalice"),
    (
        "Crimson Syndicate",
        "Karmínový syndikát",
        "Karmínový syndikát",
    ),
    (
        "Amber Syndicate",
        "Jantárový syndikát",
        "Jantarový syndikát",
    ),
    ("Violet Syndicate", "Fialový syndikát", "Fialový syndikát"),
    // Units
    ("Rifleman", "Strelec", "Střelec"),
    (
        "Cheap infantry. Strong against soldiers, weak against armor.",
        "Lacná pechota. Silná proti vojakom, slabá proti obrneným.",
        "Levná pěchota. Silná proti vojákům, slabá proti obrněným.",
    ),
    ("Scout Buggy", "Prieskumná bugina", "Průzkumná bugina"),
    (
        "Fast wheeled raider with a roof machine gun. Hunts infantry.",
        "Rýchla bugina s guľometom na streche. Loví pechotu.",
        "Rychlá bugina s kulometem na střeše. Loví pěchotu.",
    ),
    ("Battle Tank", "Bojový tank", "Bojový tank"),
    (
        "Main battle tank. Heavy cannon, thick armor.",
        "Hlavný bojový tank. Ťažký kanón, hrubý pancier.",
        "Hlavní bojový tank. Těžký kanón, silný pancíř.",
    ),
    ("Howitzer", "Húfnica", "Houfnice"),
    (
        "Long-range artillery. Splash damage, devastating against bases.",
        "Delostrelectvo s dlhým dostrelom. Plošné poškodenie, ničivé proti základniam.",
        "Dělostřelectvo s dlouhým dostřelem. Plošné poškození, ničivé proti základnám.",
    ),
    ("Harvester", "Zberač", "Sběrač"),
    (
        "Mines crystals and delivers them to a refinery. Unarmed.",
        "Ťaží kryštály a vozí ich do rafinérie. Neozbrojený.",
        "Těží krystaly a vozí je do rafinérie. Neozbrojený.",
    ),
    ("Rocketeer", "Raketometník", "Raketometčík"),
    (
        "Anti-armour infantry with a guided missile launcher. Weak against soldiers.",
        "Protitanková pechota s navádzanými raketami. Slabá proti vojakom.",
        "Protitanková pěchota s naváděnými raketami. Slabá proti vojákům.",
    ),
    ("Field Medic", "Zdravotník", "Zdravotník"),
    (
        "Unarmed. Treats wounded infantry nearby and follows the squad.",
        "Neozbrojený. Ošetruje zranenú pechotu okolo seba a ide s jednotkou.",
        "Neozbrojený. Ošetřuje zraněnou pěchotu kolem sebe a jde s jednotkou.",
    ),
    ("Engineer", "Ženista", "Ženista"),
    (
        "Unarmed. Right-click an enemy structure to capture it, or a damaged own one to repair it fully.",
        "Neozbrojený. Pravý klik na nepriateľskú budovu ju obsadí, na poškodenú vlastnú ju úplne opraví.",
        "Neozbrojený. Pravý klik na nepřátelskou budovu ji obsadí, na poškozenou vlastní ji zcela opraví.",
    ),
    ("Gunship", "Bojový vrtuľník", "Bojový vrtulník"),
    (
        "Attack helicopter with rocket pods. Flies over anything; tanks and howitzers cannot hit it.",
        "Bitevný vrtuľník s raketami. Preletí nad čímkoľvek; tanky a húfnice ho netrafia.",
        "Bitevní vrtulník s raketami. Přeletí nad čímkoli; tanky a houfnice ho netrefí.",
    ),
    ("Bulwark", "Bašta", "Bašta"),
    (
        "Heavy assault tank with twin cannons. Slow but very hard to stop.",
        "Ťažký útočný tank s dvojitým kanónom. Pomalý, ale ťažko zastaviteľný.",
        "Těžký útočný tank s dvojitým kanónem. Pomalý, ale těžko zastavitelný.",
    ),
    // Structures
    ("Command Center", "Veliteľstvo", "Velitelství"),
    (
        "Heart of the base. Lose every structure and the war is lost.",
        "Srdce základne. Prídete o všetky budovy a vojna je prehraná.",
        "Srdce základny. Přijdete o všechny budovy a válka je prohraná.",
    ),
    ("Power Plant", "Elektráreň", "Elektrárna"),
    (
        "Generates 100 power. Low power halves production speed.",
        "Vyrába 100 energie. Pri nedostatku energie sa výroba spomalí na polovicu.",
        "Vyrábí 100 energie. Při nedostatku energie se výroba zpomalí na polovinu.",
    ),
    ("Refinery", "Rafinéria", "Rafinérie"),
    (
        "Turns crystals into credits. Comes with a free harvester.",
        "Mení kryštály na kredity. Zberač je v cene.",
        "Mění krystaly na kredity. Sběrač je v ceně.",
    ),
    ("Barracks", "Kasárne", "Kasárna"),
    ("Trains infantry.", "Cvičí pechotu.", "Cvičí pěchotu."),
    ("War Factory", "Továreň", "Továrna"),
    (
        "Builds vehicles, including harvesters.",
        "Vyrába vozidlá vrátane zberačov.",
        "Vyrábí vozidla včetně sběračů.",
    ),
    ("Gun Turret", "Delová veža", "Dělová věž"),
    (
        "Static defense cannon. Good against vehicles.",
        "Pevný obranný kanón. Dobrý proti vozidlám.",
        "Pevný obranný kanón. Dobrý proti vozidlům.",
    ),
    ("Wall", "Múr", "Zeď"),
    (
        "Concrete barrier. Drag to lay a line; each segment is paid as it is placed.",
        "Betónová bariéra. Ťahaním postavíte rad; každý diel sa platí pri položení.",
        "Betonová bariéra. Tažením postavíte řadu; každý díl se platí při položení.",
    ),
    ("Helipad", "Heliport", "Heliport"),
    ("Season", "Ročné obdobie", "Roční období"),
    ("Map size", "Veľkosť mapy", "Velikost mapy"),
    (
        "{} ({} x {} cells)",
        "{} ({} x {} polí)",
        "{} ({} x {} polí)",
    ),
    ("Standard", "Štandardná", "Standardní"),
    ("Large", "Veľká", "Velká"),
    ("Huge", "Obrovská", "Obrovská"),
    (
        "Check for updates",
        "Hľadať aktualizácie",
        "Hledat aktualizace",
    ),
    (
        "Version {} is out - download it",
        "Vyšla verzia {} - stiahnuť",
        "Vyšla verze {} - stáhnout",
    ),
    (
        "Downloading {}... {}%",
        "Sťahujem {}... {}%",
        "Stahuji {}... {}%",
    ),
    (
        "Saved to {} - click to show it",
        "Uložené do {} - kliknutím zobrazíte",
        "Uloženo do {} - kliknutím zobrazíte",
    ),
    (
        "Update failed: {}",
        "Aktualizácia zlyhala: {}",
        "Aktualizace selhala: {}",
    ),
    ("Summer", "Leto", "Léto"),
    ("Autumn", "Jeseň", "Podzim"),
    ("Winter", "Zima", "Zima"),
    ("Repair Bay", "Opravovňa", "Opravna"),
    (
        "Repairs vehicles that wait next to it.",
        "Opravuje vozidlá, ktoré čakajú vedľa nej.",
        "Opravuje vozidla, která čekají vedle ní.",
    ),
    ("Sniper", "Ostreľovač", "Odstřelovač"),
    (
        "Marksman who drops infantry from far away. Useless against armour.",
        "Strelec, ktorý zloží pechotu zďaleka. Proti pancieru je zbytočný.",
        "Střelec, který složí pěchotu zdálky. Proti pancíři je zbytečný.",
    ),
    (
        "Tech Center",
        "Technologické centrum",
        "Technologické centrum",
    ),
    (
        "Research labs. Unlocks the Bulwark, the Howitzer and the Gunship.",
        "Výskumné laboratóriá. Sprístupní Bulwark, húfnicu a bitevný vrtuľník.",
        "Výzkumné laboratoře. Zpřístupní Bulwark, houfnici a bitevní vrtulník.",
    ),
    (
        "Flak Cannon",
        "Protilietadlový kanón",
        "Protiletadlový kanón",
    ),
    (
        "Anti-aircraft gun pit. Cannot shoot at ground targets.",
        "Protilietadlové postavenie. Na pozemné ciele strieľať nevie.",
        "Protiletadlové postavení. Na pozemní cíle střílet neumí.",
    ),
    ("Flak Track", "Protilietadlový tank", "Protiletadlový tank"),
    (
        "Mobile anti-aircraft gun. Shreds gunships, weak against armour.",
        "Pojazdný protilietadlový kanón. Rozstrieľa vrtuľníky, proti pancieru je slabý.",
        "Pojízdný protiletadlový kanón. Rozstřílí vrtulníky, proti pancíři je slabý.",
    ),
    (
        "Builds gunships and repairs aircraft hovering over the pad.",
        "Vyrába bitevné vrtuľníky a opravuje lietadlá visiace nad plochou.",
        "Vyrábí bitevní vrtulníky a opravuje letadla visící nad plochou.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_translation_keeps_its_placeholders() {
        let holes = |text: &str| text.matches("{}").count();
        for &(english, slovak, czech) in TEXTS {
            assert_eq!(holes(english), holes(slovak), "Slovak: {english}");
            assert_eq!(holes(english), holes(czech), "Czech: {english}");
        }
        assert_eq!(
            translations().len(),
            TEXTS.len(),
            "an English text is listed twice"
        );
    }

    #[test]
    fn templates_fill_in_order_and_unknown_texts_stay_english() {
        set_language(Language::English);
        assert_eq!(tr_fmt("POWER {} / {}", &[&40, &120]), "POWER 40 / 120");
        assert_eq!(tr("No such text"), "No such text");
    }
}
