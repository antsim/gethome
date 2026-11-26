# Työaikalaskuri – toteutussuunnitelma

Henkilökohtainen macOS-sovellus, joka hakee työaikakirjaukset Toggl Trackista ja näyttää reaaliajassa, paljonko päivän työajasta on jäljellä. Kun Togglessa ei ole ajastinta käynnissä, laskuri pysähtyy ja näytölle tulee teksti **"Tauolla"**.

Tämä dokumentti on toteutuksen lähde. Lisää viittaus siihen repositoryn `CLAUDE.md`-tiedostoon.

---

## 1. Tavoitteet ja rajaus

**Tavoitteet**
- Jäljellä oleva työaika näkyy MacBookin näytöllä koko ajan menu barissa.
- Lisäksi on pieni ikkuna, joka pysyy muiden päällä. Sen voi siirtää ulkoiselle näytölle, ja sijainti säilyy.
- Laskuri päivittyy sekunnin välein ilman, että Toggl API:a kutsutaan joka sekunti.
- Tauko tunnistetaan siitä, että Togglessa ei ole käynnissä olevaa ajastinta.

**Ei tässä versiossa**
- Viikon tai kuukauden liukumasaldo (mahdollinen jatkokehitys, ks. luku 11).
- Useamman käyttäjän tuki ja raportit.
- Windows- tai Linux-tuki. Tauri mahdollistaa ne myöhemmin, mutta niitä ei testata.

---

## 2. Teknologiat

| Osa | Valinta | Perustelu |
|---|---|---|
| Sovelluskehys | **Tauri 2** (uusin vakaa) | Pieni binääri, natiivi tray-ikoni tekstillä, `alwaysOnTop`-ikkuna |
| Ydinlogiikka | **Rust** (Tauri-backend) | Ajastus, API-kutsut ja tila. Ks. perustelu alla |
| Käyttöliittymä | **Angular** (uusin vakaa, standalone components, signals) | Kehittäjän vahva osaamisalue |
| HTTP | `reqwest` (Rust) | Ei CORS-ongelmia, kutsut eivät kulje webviewn kautta |
| Async/ajastus | `tokio` | Sekuntitikki ja pollaus |
| API-tokenin säilytys | `keyring`-crate → macOS Keychain | Tokenia ei tallenneta tiedostoon |
| Muut asetukset | `tauri-plugin-store` | Päivätavoite, näyttöasetukset |
| Ikkunan sijainti | `tauri-plugin-window-state` | Sijainti säilyy myös ulkoisella näytöllä |
| Käynnistys kirjautuessa | `tauri-plugin-autostart` | Valinnainen asetus |
| Aika | `chrono` + `chrono-tz` | Päivän raja paikallisessa aikavyöhykkeessä |

### Miksi logiikka on Rustissa eikä Angularissa
Menu barin tekstin pitää päivittyä myös silloin, kun ikkuna on piilossa. macOS (App Nap / WKWebView) hidastaa piilossa olevan webviewn ajastimia, joten sekuntitikki ei ole siellä luotettava. Siksi:

- **Rust** omistaa tilan: tikittää, pollaa Togglea, laskee ajat, päivittää tray-tekstin ja lähettää tilan käyttöliittymälle eventteinä.
- **Angular** on pelkkä näkymä. Se kuuntelee `timer-state`-eventtiä, näyttää tilan ja kutsuu Rust-komentoja (`invoke`).

Rust-osuus on pieni (arviolta muutama sata riviä), joten kynnys on matala.

---

## 3. Toggl API ja rajoitukset

### Rajoitukset – arkkitehtuurin tärkein reunaehto
- Käyttäjäkohtaiset endpointit (`/me/...`) sallivat **30 pyyntöä tunnissa** liukuvassa ikkunassa kaikilla plaaneilla (Enterprise poikkeus).
- Workspace-kohtaiset: Free 30/h, Starter 240/h, Premium 600/h.
- Ylityksestä tulee **HTTP 402** (kvootti) tai **HTTP 429** (purskeraja, noin 1 req/s).

➡️ **Pyyntöbudjetti: enintään 20 pyyntöä tunnissa normaalikäytössä.** Loput jäävät käyttäjän omille Toggl-sovelluksille ja manuaalisille päivityksille.

### Käytettävät endpointit
Perus-URL: `https://api.track.toggl.com/api/v9`
Autentikointi: HTTP Basic, käyttäjänimi = API-token, salasana = `api_token`.

| Tarkoitus | Kutsu | Milloin |
|---|---|---|
| Käyttäjä ja oletus-workspace | `GET /me` | Kerran tokenin asetuksen yhteydessä. `default_workspace_id` välimuistiin |
| Päivän kirjaukset | `GET /me/time_entries?start_date=<päivän alku>&end_date=<huomisen alku>` | Käynnistyessä, tilamuutoksessa ja 30 min välein |
| Käynnissä oleva ajastin | `GET /me/time_entries/current` (palauttaa `null`, jos ajastin ei käy) | 4 min välein |
| Ajastimen pysäytys | `PATCH /workspaces/{wid}/time_entries/{id}/stop` | Käyttäjän toiminto sovelluksesta |
| Ajastimen käynnistys | `POST /workspaces/{wid}/time_entries` (`duration: -1`, `start: now`, `created_with: "tyoaikalaskuri"`, `workspace_id`) | Käyttäjän toiminto sovelluksesta. Jatkaa viimeisimmän kirjauksen kuvauksella ja projektilla |

Huomioita datasta:
- Käynnissä olevalla kirjauksella `stop` on `null` ja `duration` negatiivinen. Kesto lasketaan aina `now - start`, ei `duration`-kentästä.
- Keskiyön yli menevät kirjaukset rajataan kuluvaan päivään (paikallinen aikavyöhyke, oletus `Europe/Helsinki`).
- Päivämäärät lähetetään RFC3339-muodossa aikavyöhykkeen kanssa.

### Pollausstrategia
| Tapahtuma | Toiminto | Pyyntöjä/h |
|---|---|---|
| 4 min ajastin | `GET /me/time_entries/current` | 15 |
| 30 min ajastin | Päivän kirjausten täysi haku (korjaa mahdollisen ajautumisen) | 2 |
| Current-kirjauksen `id` vaihtui tai ajastin pysähtyi | Päivän kirjausten täysi haku | tarpeen mukaan |
| Herätys unesta (tikkien väli > 60 s) tai päivä vaihtui | Täysi haku | tarpeen mukaan |
| Käyttäjän "Päivitä"-painike | Täysi haku + current. Throttle: enintään kerran 30 sekunnissa | tarpeen mukaan |

- Sovellus pitää omaa liukuvaa laskuria tehdyistä pyynnöistä. Jos tunnin aikana on tehty yli 25 pyyntöä, pollausväli harvenee automaattisesti 10 minuuttiin.
- **402/429**: ei uudelleenyritystä heti. Eksponentiaalinen backoff (1 min → 2 → 5 → 10 min). Laskuri jatkaa viimeisimmällä tunnetulla tilalla, ja UI:ssa näkyy pieni varoitus "Toggl-yhteys rajoitettu".
- Verkkovirhe: sama backoff, ja UI näyttää "Offline – viimeksi päivitetty klo HH:MM".
- Kun käyttäjä pysäyttää tai käynnistää ajastimen **tämän sovelluksen kautta**, tila päivittyy heti optimistisesti. Jos ajastin pysäytetään Toggl-sovelluksessa, muutos näkyy viimeistään 4 minuutin viiveellä. Viive on hyväksytty.

---

## 4. Tilamalli ja laskenta

```rust
enum TimerStatus {
    NotStarted,   // tänään ei kirjauksia
    Working,      // ajastin käynnissä
    OnBreak,      // ei käynnissä olevaa ajastinta, mutta tänään on kirjauksia
    Done,         // tavoite täynnä (ajastin voi silti käydä → ylityö)
}

struct DayState {
    completed_secs: i64,             // päivän päättyneiden kirjausten summa (rajattuna tähän päivään)
    running_start: Option<DateTime>, // käynnissä olevan kirjauksen alku
    running_entry_id: Option<i64>,
    last_stop: Option<DateTime>,     // viimeisimmän kirjauksen loppu → tauon kesto
    last_synced: DateTime,
}
```

Laskenta joka sekunti:
```
worked     = completed_secs + (running ? now - running_start : 0)
remaining  = target_secs - worked          // negatiivinen = ylityö
eta        = running ? now + remaining : None   // "Valmis klo 16:05"
break_secs = OnBreak ? now - last_stop : 0
```

Laskentalogiikka toteutetaan **puhtaana funktiona** (`fn compute(state, now, settings) -> ViewModel`), jotta sen voi yksikkötestata ilman verkkoa tai kelloa.

### Event käyttöliittymälle (`timer-state`, joka sekunti)
```ts
interface TimerViewModel {
  status: 'NotStarted' | 'Working' | 'OnBreak' | 'Done';
  remainingSecs: number;   // voi olla negatiivinen
  workedSecs: number;
  targetSecs: number;
  breakSecs: number;
  eta: string | null;      // ISO, vain kun Working
  lastSynced: string;
  syncWarning: 'none' | 'rateLimited' | 'offline' | 'authError';
}
```

---

## 5. Käyttöliittymä

### Menu bar (tray)
- Teksti päivittyy joka sekunti `TrayIcon::set_title`-kutsulla:
  - Working: `⏱ 3:42:15` (sekunnit valinnaisia; asetus "Näytä sekunnit")
  - OnBreak: `⏸ 3:42 Tauolla`
  - NotStarted: `⏱ 7:30` (koko tavoite)
  - Done / ylityö: `✓ +0:12`
- Klikkaus avaa valikon: *Näytä/piilota ikkuna*, *Pysäytä ajastin* / *Jatka työtä*, *Päivitä nyt*, *Asetukset…*, *Lopeta*.
- Sovellus ei näy Dockissa: `ActivationPolicy::Accessory`.

### Kelluva ikkuna
- Kehyksetön (`decorations: false`), läpinäkyvä tausta (`transparent: true` + `macOSPrivateApi: true`), `alwaysOnTop: true`, `skipTaskbar: true`, näkyy kaikilla Spaceilla (`visibleOnAllWorkspaces: true`).
- Oletuskoko noin 260 × 120 px. Ikkunaa voi raahata mistä tahansa kohdasta (`data-tauri-drag-region`).
- Sisältö:
  - Iso jäljellä oleva aika `3:42:15`
  - Tilarivi: "Työssä · valmis klo 16:05" / **"Tauolla"** + tauon kesto / "Ei aloitettu" / "Tavoite täynnä +0:12"
  - Ohut edistymispalkki (tehty / tavoite)
  - Pieni painike: ⏸ / ▶ (pysäytä tai jatka ajastinta)
- Tauolla: aika on harmaa ja pysähtynyt, ja "Tauolla"-teksti on selvästi esillä. Ylityö näytetään eri värillä.
- Seuraa macOS:n vaaleaa ja tummaa teemaa (`prefers-color-scheme`).
- Sijainti ja koko tallentuvat (`window-state`). Jos tallennettu näyttö ei ole kytkettynä, ikkuna palautetaan päänäytölle.
- Asetus "Läpinäkyvyys" (esim. 60–100 %).

### Asetukset-ikkuna (erillinen tavallinen ikkuna)
- Toggl API -token (tallennus Keychainiin). Painike "Testaa yhteys" kutsuu `/me` ja näyttää nimen ja workspacen.
- Päivän tavoite (oletus 7 h 30 min). Mahdollisuus eri tavoitteeseen viikonpäivittäin (esim. pe 6 h).
- Aikavyöhyke (oletus järjestelmän).
- Näytä sekunnit menu barissa (kyllä/ei).
- Käynnistä kirjautuessa (autostart).
- Ikkunan läpinäkyvyys.

---

## 6. Projektirakenne

```
/
├── CLAUDE.md                 # viittaus tähän suunnitelmaan + konventiot
├── PLAN.md                   # tämä tiedosto
├── src/                      # Angular
│   ├── app/
│   │   ├── timer/            # kelluvan ikkunan komponentti
│   │   ├── settings/         # asetukset-ikkuna
│   │   └── core/
│   │       ├── timer-state.service.ts   # kuuntelee `timer-state`-eventtiä → signal
│   │       └── tauri-commands.ts        # tyypitetyt invoke-wrapperit
│   └── styles.scss
└── src-tauri/
    ├── src/
    │   ├── main.rs
    │   ├── lib.rs            # Tauri-setup, tray, ikkunat, komennot
    │   ├── toggl/
    │   │   ├── client.rs     # reqwest-client, auth, virheiden mappaus (402/429/401)
    │   │   ├── models.rs     # serde-mallit
    │   │   └── rate_budget.rs# liukuva pyyntölaskuri
    │   ├── engine/
    │   │   ├── state.rs      # DayState, TimerStatus
    │   │   ├── compute.rs    # puhdas laskenta → ViewModel
    │   │   └── scheduler.rs  # tikki, pollaus, backoff, wake-tunnistus
    │   ├── settings.rs       # store + keyring
    │   └── tray.rs           # tray-tekstin muotoilu ja valikko
    ├── capabilities/
    └── tauri.conf.json
```

### Tauri-komennot (Rust → `invoke`)
- `get_state() -> TimerViewModel`
- `refresh_now()`
- `stop_timer()` / `resume_timer()`
- `get_settings()` / `save_settings(settings)`
- `set_api_token(token) -> Result<UserInfo>` (validoi `/me`:llä, tallentaa Keychainiin)
- `toggle_window()`

---

## 7. Toteutusvaiheet

Jokainen vaihe on oma commit tai PR, ja se toimii itsenäisesti.

1. **Runko**: Tauri 2 + Angular -projekti, sovellus käynnistyy, tray-ikoni näkyy, Dock-ikoni piilossa.
2. **Laskentaydin**: `state.rs` + `compute.rs` + kattavat yksikkötestit (ks. luku 8). Ei verkkoa.
3. **Toggl-client**: `client.rs`, mallit, virheiden käsittely, `rate_budget.rs`. Testit mockatulla HTTP:llä (esim. `wiremock`).
4. **Scheduler**: sekuntitikki, pollaus, täysi haku tilamuutoksessa, backoff, wake- ja päivänvaihtotunnistus. Tray-teksti päivittyy.
5. **Asetukset**: token Keychainiin, tavoite ja muut asetukset storeen, asetusikkuna Angularissa.
6. **Kelluva ikkuna**: Angular-näkymä, `timer-state`-event → signal, ulkoasu, raahaus, `window-state`, alwaysOnTop.
7. **Ajastimen ohjaus**: pysäytä/jatka sovelluksesta (optimistinen päivitys + tarkistushaku).
8. **Viimeistely**: autostart, teemat, läpinäkyvyys, virhetilojen UI, sovelluksen ikoni, `.app`/`.dmg`-build.

---

## 8. Testaus

**Yksikkötestit (Rust, `compute.rs`)**
- Ei kirjauksia → `NotStarted`, remaining = tavoite.
- Yksi käynnissä oleva kirjaus → remaining pienenee sekunnilla, kun `now` kasvaa sekunnilla.
- Päättyneitä kirjauksia + ei käynnissä → `OnBreak`, remaining ei muutu ajan kuluessa, `break_secs` kasvaa.
- Tavoite ylittyy → `Done`, negatiivinen remaining, muotoilu `+0:12`.
- Keskiyön yli menevä kirjaus rajataan oikein.
- Kesäaikasiirtymän päivä (päivän pituus 23 h / 25 h) ei riko laskentaa.
- Viikonpäiväkohtainen tavoite valitaan oikein.

**Integraatiotestit (Rust, mockattu Toggl)**
- 402 → backoff käynnistyy, ei uusintakutsua ennen viivettä, viimeisin tila säilyy.
- 429 → sama.
- 401 → `authError`, pollaus pysähtyy, kunnes token vaihdetaan.
- Current-`id` vaihtuu → tehdään täysi haku.
- Pyyntöbudjetti: simuloidun tunnin aikana alle 25 pyyntöä normaalikäytössä.

**Manuaalinen tarkistuslista**
- Ikkuna pysyy kokoruutusovellusten ja muiden ikkunoiden päällä.
- Ikkunan siirto ulkoiselle näytölle → sovelluksen uudelleenkäynnistys → ikkuna palaa samaan paikkaan.
- Ulkoinen näyttö irrotetaan → ikkuna siirtyy päänäytölle.
- Kone nukkuu 30 min → herätessä tila päivittyy heti.
- Ajastimen pysäytys Toggl-sovelluksessa → "Tauolla" näkyy viimeistään 4 minuutin päästä.
- Ajastimen pysäytys tästä sovelluksesta → "Tauolla" näkyy heti.

---

## 9. Hyväksymiskriteerit

- Menu barin teksti päivittyy sekunnin välein myös silloin, kun kelluva ikkuna on piilossa.
- Tauolla aika on pysähtynyt ja "Tauolla" näkyy sekä menu barissa että ikkunassa.
- Normaalikäytössä Toggl API -kutsuja on enintään 20 tunnissa.
- API-token on vain macOS Keychainissa.
- Sovellus selviää verkkokatkoista, unesta ja API-rajoista kaatumatta, ja käyttäjä näkee, milloin data on viimeksi päivitetty.

---

## 10. Konventiot Claude Codelle

- Rust: `cargo fmt` + `cargo clippy -- -D warnings` puhtaana. Virheet `thiserror`-tyypeillä, ei `unwrap()`-kutsuja tuotantokoodissa.
- Angular: standalone components, signals, ei NgModuleja. Strict TypeScript.
- Kaikki Toggl-kutsut kulkevat `toggl/client.rs`:n kautta ja kirjataan `rate_budget`iin. Suoria HTTP-kutsuja muualta ei tehdä.
- Laskentalogiikka pysyy puhtaana (ei I/O:ta, `now` annetaan parametrina).
- UI-tekstit ovat suomeksi, koodi ja kommentit englanniksi.

---

## 11. Jatkokehitysideoita (ei tässä versiossa)

- Liukumasaldo viikon/kuukauden yli (Reports API, harva haku esim. kerran päivässä).
- Ilmoitus, kun tavoite täyttyy tai tauko venyy yli X minuutin.
- Toggl-webhookit reaaliaikaiseen tilamuutokseen. Vaatii julkisen vastaanottopisteen (esim. pieni pilvifunktio + SSE), joten ei kannata henkilökohtaisessa käytössä ennen kuin pollausviive oikeasti haittaa.
- Taukojen erittely päivän aikajanana.

---

## 12. Toteutuksen tarkennukset

Suunnitelmaa tarkennettiin toteutuksen aikana seuraavasti.

**Käyttäjän päätökset**
- Työajaksi lasketaan **kaikki** käyttäjän kirjaukset kaikista workspaceista.
- Oletustavoite on ma–pe 7:30 ja la–su 0 h. Kaikki päivät ovat muokattavissa. Jos tavoite on 0 h ja kirjauksia on, kaikki tehty aika näkyy ylityönä.
- "Jatka työtä" jatkaa viimeisimmän kirjauksen kuvauksella, projektilla, tehtävällä ja tageilla. Jos tänään ei ole kirjauksia, viimeisin kirjaus haetaan aiemmilta päiviltä (`GET /me/time_entries` ilman päivämääriä). Jos sellaistakaan ei ole, ajastin käynnistetään ilman kuvausta oletus-workspaceen.

**Tekniset tarkennukset**
- Päivän haku alkaa **edellisen päivän alusta**, koska Toggl suodattaa kirjaukset alkuajan mukaan. Näin keskiyön yli menevät kirjaukset saadaan mukaan, ja ne rajataan kuluvaan päivään.
- Täysi haku sisältää myös käynnissä olevan kirjauksen, joten erillistä `current`-kutsua ei tehdä täyden haun yhteydessä.
- `last_synced` on erillisessä `SyncStatus`-rakenteessa eikä `DayState`ssa.
- `syncWarning` sisältää lisäksi arvon `noToken` (tokenia ei ole asetettu).
- `TimerViewModel` sisältää lisäksi kentän `running`, jota käytetään ⏸/▶-painikkeessa ja tray-valikossa.
- ETA (`eta`) annetaan vain tilassa `Working`, ei ylityön aikana.
- Asetukset on jaettu kolmeen tiedostoon: `settings/mod.rs` (malli ja validointi), `settings/persistence.rs` (store) ja `settings/secrets.rs` (Keychain).
- Kelluvan ikkunan näkyvyys tallennetaan omiin asetuksiin (`timerWindowVisible`). `window-state` tallentaa vain sijainnin ja koon.
- Pysäytyksen ja käynnistyksen jälkeen tehdään tarkistushaku seuraavalla sekuntitikillä.
- Uusia komentoja: `clear_api_token`, `test_connection`, `open_settings` ja `list_timezones`.

**Toggl 2.0 -tuki** (lisätty käyttöönoton jälkeen)

Toggl 2.0 (`focus.toggl.com`) on eri tuote kuin Toggl Track, ja sillä on oma API. Track API hylkää sen avaimet. Molempia tuetaan, ja versio tunnistetaan avaimesta: `toggl_sk_…` on Toggl 2.0, muut ovat Track-tokeneita. Dokumentaatio: https://engineering.toggl.com/docs/focus/

| Tarkoitus | Toggl 2.0 -kutsu |
|---|---|
| Avaimen tarkistus, organisaatio ja workspace | `GET /users/me/landing-workspace` (välimuistiin kerran per avain) |
| Päivän kirjaukset | `GET /organizations/{org}/workspaces/{ws}/time-entries/stream?date_from&date_to&include_taskless=true` (yksi pyyntö, ei sivutusta) |
| Käynnissä oleva ajastin | `GET …/tracking/current` (204 = ei ajastinta) |
| Käynnistys | `POST …/tracking/start` (`type: "activity"`) |
| Pysäytys | `POST …/tracking/stop`, valinnainen body `{"end": "<RFC 3339>"}` (404 = jo pysäytetty) |
| Viimeisin kirjaus jatkamista varten | `GET …/time-entries?order_by=-start&per_page=20` |

- Autentikointi: `Authorization: Bearer toggl_sk_…`.
- Kiintiö on per käyttäjä per organisaatio: Free 30/h, Starter 240/h, Premium 600/h. Ylitys palauttaa HTTP 402:n. Pyyntöbudjetti ja pollausstrategia pysyvät ennallaan.
- Kirjauksilla ei ole `stop`-kenttää. Loppu on `start + duration`, ja käynnissä olevalta puuttuu `duration`.
- Työajaksi ei lasketa tauko-tyyppisiä (`type: break`), pelkästään suunniteltuja (ei `start`ia) eikä poistettuja kirjauksia. Käynnissä oleva tauko tarkoittaa tilaa *Tauolla*.
- Toggl 2.0 ei kerro käyttäjän nimeä, joten asetuksissa näytetään tuote ja workspace.
- Sovellus lukee yhden workspacen eli sen, jonka `landing-workspace` palauttaa.
- Yleinen korjaus: jos ajastin on käynnistetty ennen haettua aikaväliä, `current`-kirjaus otetaan tilaan suoraan, jotta seuraava pollaus ei käynnistä uutta täyttä hakua.

**Päivän täyttäminen tauolle siirryttäessä**

- Jos tauko (⏸) painetaan, kun päivän tavoitteesta on jäljellä **alle 15 min**, käynnissä oleva kirjaus päätetään tulevaisuuteen eli siihen hetkeen, jolloin tavoite täyttyy. Näin koneen voi sulkea hieman etuajassa ja päivä on silti täysi. Jos aikaa on 15 min tai enemmän tai tavoite on jo täynnä, ajastin pysäytetään heti normaalisti.
- Toggl Trackissa tämä tehdään kutsulla `PUT /workspaces/{wid}/time_entries/{id}` (`stop` + `duration`), Toggl 2.0:ssa kutsulla `POST …/tracking/stop` bodylla `{"end": "<aika>"}`. Pyyntöjä kuluu yhtä monta kuin tavallisessa pysäytyksessä.
- Jos Toggl hylkää loppuajan (HTTP 4xx, esim. jos tulevaa aikaa ei sallita), ajastin pysäytetään heti tavalliseen tapaan.
- Jos työtä jatketaan ennen tulevaa loppuaikaa, Toggl Trackissa edellisen kirjauksen loppu siirretään ensin jatkamishetkeen (yksi ylimääräinen `PUT`), jotta kirjaukset eivät mene päällekkäin. Toggl 2.0:ssa päättyneen kirjauksen muokkauskutsua ei ole vielä otettu käyttöön, joten siellä uusi kirjaus vain käynnistetään.
- Raja on vakio `FINISH_DAY_WINDOW_SECS` (`engine/compute.rs`).

**Päivän kirjaukset (tuntikirjausten kopiointi)**

Päivän päätteeksi tunnit kirjataan Togglesta yrityksen omaan järjestelmään. Kelluva ikkuna laajenee yhteenvedoksi, josta kirjaukset voi kopioida suoraan.

- Avautuminen:
  - automaattisesti kerran päivässä, kun tavoite täyttyy (asetus *Näytä päivän kirjaukset automaattisesti*, oletuksena päällä)
  - ⏹-painikkeesta (*Lopeta työpäivä*), joka pysäyttää ajastimen (päivän täyttölogiikka mukana) ja avaa yhteenvedon
  - 📋-painikkeesta ja tray-valikon kohdista *Lopeta työpäivä* ja *Päivän kirjaukset…*
- Ikkuna pysyy muiden päällä, joten kirjauksia voi kopioida yksi kerrallaan selaimeen. Sulkeminen (✕ tai Esc) palauttaa ikkunan käyttäjän omaan kokoon. Jos laajennettu ikkuna ei mahdu näytölle, sitä siirretään, ja siirto perutaan sulkiessa.
- Taski on kaikki saman kuvauksen (trimmattuna) kirjaukset. Kirjaukset ilman kuvausta ovat yksi taski "(ei kuvausta)".
- Pyöristys (`engine/summary.rs`, puhdas funktio):
  1. Kokonaismäärä on tehty aika pyöristettynä **lähimpään** puoleen tuntiin (tasan 15 min pyöristyy ylös).
  2. Jokainen taski saa ensin aikansa alaspäin pyöristettynä puoleen tuntiin. Puuttuvat puolituntiset jaetaan suurimman jäännöksen menetelmällä (tasatilanteessa pidempi taski, sitten aiempi). Rivit ovat aina tasa- tai puolitunteja, ja niiden summa on kokonaismäärä.
  3. Nollaan pyöristyvät taskit (tyypillisesti alle 15 min) jätetään pois.
- Rivit ovat siinä järjestyksessä, jossa taskeja on tehty. Rivillä näkyy myös todellinen Toggl-aika.
- Rivin klikkaus kopioi `tunnit<TAB>kuvaus`, esim. `1,5` + tab + `Code review` (desimaalipilkku). *Kopioi kaikki* kopioi kaikki rivit rivinvaihdoin. Kopioidut rivit merkitään ✓:llä. Merkintä poistuu, jos rivin tunnit muuttuvat.
- Leikepöytä: `tauri-plugin-clipboard-manager`, jota kutsutaan Rustista komennolla `copy_to_clipboard`.
- Uusia API-kutsuja ei tarvita. `DayState` säilyttää päivän päättyneet kirjaukset (`segments`) ja käynnissä olevan kirjauksen kuvauksen.
- Päivän tavoite on aina tasa- tai puolituntinen. Validointi hylkää muut arvot, ja vanhat tallennetut arvot pyöristetään lähimpään puoleen tuntiin.
- Uusia komentoja: `get_day_summary`, `is_summary_open`, `show_summary`, `close_summary`, `finish_day` ja `copy_to_clipboard`. Event `summary-visibility` kertoo käyttöliittymälle, kumpi näkymä on auki.
