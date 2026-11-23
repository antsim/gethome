# Työaikalaskuri

Henkilökohtainen macOS-sovellus, joka näyttää menu barissa ja pienessä
kelluvassa ikkunassa, paljonko päivän työajasta on jäljellä. Tiedot haetaan
[Togglista](https://toggl.com/) (Toggl 2.0 tai Toggl Track). Kun Togglessa ei ole ajastinta
käynnissä, laskuri pysähtyy ja näytölle tulee teksti **"Tauolla"**.

```
⏱ 3:42:15            menu bar, ajastin käynnissä
⏸ 3:42 Tauolla       ei käynnissä olevaa ajastinta
✓ +0:12              tavoite täynnä, ylityötä 12 min
```

## Ominaisuudet

- Jäljellä oleva aika menu barissa. Päivittyy sekunnin välein myös silloin, kun ikkuna on piilossa.
- Kehyksetön, aina päällä pysyvä ikkuna. Sen voi raahata esimerkiksi ulkoiselle näytölle, ja sijainti säilyy.
- Ajastimen pysäytys ja jatkaminen suoraan sovelluksesta.
- Päiväkohtainen tavoite (oletus ma–pe 7:30, la–su 0 h).
- Valmistumisaika-arvio ("valmis klo 16:05") ja tauon kesto.
- Vaalea ja tumma teema, säädettävä läpinäkyvyys, käynnistys kirjautuessa.
- Toggl API -kutsuja normaalikäytössä alle 20 tunnissa. Rajoitukset, verkkokatkot ja uni käsitellään hallitusti.
- API-avain tallennetaan vain macOS:n avainnippuun (Keychain).

## Asennus ja käyttöönotto

Vaatimukset: macOS 11+, [Node.js](https://nodejs.org/) 22+, [Rust](https://rustup.rs/) (stable) ja Xcode Command Line Tools.

```bash
git clone https://github.com/antsim/gethome.git
cd gethome
npm install
```

Kehitystilassa ajo:

```bash
npm run dev
```

Julkaisuversion (`.app` ja `.dmg`) rakentaminen:

```bash
npm run bundle
```

Valmis paketti löytyy hakemistosta `src-tauri/target/release/bundle/`.
Sovellus ei ole allekirjoitettu, joten ensimmäisellä käynnistyskerralla se
avataan Finderissa hiiren oikealla painikkeella → *Avaa*.

### API-avaimen asetus

Sovellus tukee sekä **Toggl 2.0:aa** (`focus.toggl.com`) että vanhempaa
**Toggl Trackia** (`track.toggl.com`). Versio tunnistetaan avaimesta
automaattisesti.

- **Toggl 2.0:** *Settings → Toggl account → API key*. Avain alkaa
  `toggl_sk_` ja näytetään vain kerran. Uuden avaimen luominen mitätöi
  vanhan.
- **Toggl Track:** [profiilisivun](https://track.toggl.com/profile) kohta
  *API Token*.

1. Avaa sovelluksen valikosta menu barissa *Asetukset…*.
2. Liitä avain ja paina *Tallenna*. Sovellus tarkistaa avaimen ja näyttää
   yhdistetyn tilin ja workspacen.

Avain tallennetaan vain avainnippuun. Sitä ei kirjoiteta tiedostoon eikä
lähetetä mihinkään muualle kuin Toggl API:in.

Toggl 2.0:ssa sovellus lukee yhden workspacen kirjaukset: sen, jonka Toggl
antaa avaimelle oletukseksi (`users/me/landing-workspace`). Tauko-tyyppiset
kirjaukset (*break*) ja pelkästään suunnitellut kirjaukset eivät kerrytä
työaikaa.

## Käyttö

| Toiminto | Miten |
|---|---|
| Näytä/piilota ikkuna | Menu bar → *Näytä ikkuna* / *Piilota ikkuna* |
| Pysäytä ajastin / jatka työtä | Ikkunan ⏸/▶-painike tai menu bar |
| Päivitä heti | Menu bar → *Päivitä nyt* (enintään kerran 30 sekunnissa) |
| Asetukset | Menu bar → *Asetukset…* tai ikkunan ⚙ (näkyy hiiren ollessa ikkunan päällä) |
| Siirrä ikkunaa | Raahaa mistä tahansa kohdasta |

Jos ajastin pysäytetään Toggl-sovelluksessa, muutos näkyy enintään neljän
minuutin viiveellä. Tämä johtuu Toggl API:n pyyntörajoista (Free-tilillä 30
pyyntöä tunnissa). Tämän sovelluksen kautta tehdyt muutokset näkyvät heti.

## Vianetsintä

**`failed to run 'cargo metadata' … No such file or directory`**
Rust puuttuu tai `cargo` ei ole PATHissa. Asenna Rust ja lataa ympäristö:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
cargo --version
```

Varmista myös, että Xcode Command Line Tools on asennettu (`xcode-select --install`).

**`npm warn allow-scripts …`**
Uudet npm-versiot eivät aja pakettien asennusskriptejä ilman hyväksyntää.
Varoitus ei yleensä estä kehitystä. Jos build valittaa esbuildista, hyväksy
sen skripti ja asenna uudelleen:

```bash
npm approve-scripts esbuild
npm install
```

## Kehitys

```bash
npm test -- --watch=false     # Angular-testit (Vitest)
npm run format:check          # Prettier
npm run build                 # Angular-build (tarvitaan ennen cargo-komentoja)

cd src-tauri
cargo test                    # Rust-testit (laskenta + mockattu Toggl API)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Linuxilla Rust-koodin kääntäminen vaatii Taurin
[järjestelmäriippuvuudet](https://tauri.app/start/prerequisites/#linux)
(esim. `libwebkit2gtk-4.1-dev`). Sovellusta tuetaan ja testataan vain
macOS:llä.

Arkkitehtuuri ja koodikonventiot on kuvattu tiedostossa
[CLAUDE.md](CLAUDE.md). Alkuperäinen suunnitelma ja toteutuksen tarkennukset
ovat tiedostossa [PLAN.md](PLAN.md).

### Projektin rakenne

```
src/              Angular-käyttöliittymä (kelluva ikkuna ja asetukset)
src-tauri/        Rust-backend: tila, ajastus, Toggl-client, tray
PLAN.md           Toteutussuunnitelma
CLAUDE.md         Arkkitehtuuri ja konventiot
```

## Tietosuoja

- Sovellus keskustelee vain Toggl API:n kanssa (`focus.toggl.com` tai `api.track.toggl.com`).
- API-avain on vain macOS:n avainnipussa.
- Asetukset (tavoitteet, näyttöasetukset sekä Toggl-tilin tiedot ja workspace) tallennetaan sovelluksen datahakemistoon (`~/Library/Application Support/io.github.antsim.tyoaikalaskuri/`).

## Lisenssi

[MIT](LICENSE)
