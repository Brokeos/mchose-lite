<div align="center">

# MCHOSE Lite

**Un remplaçant léger de MCHOSE HUB pour les souris MCHOSE, écrit en Rust.**

DPI, polling rate jusqu'à 8 kHz, capteur, profils embarqués et batterie dans la zone de
notification, dans **~2 Mo de RAM** au lieu de ~110 Mo.

![Windows](https://img.shields.io/badge/Windows-10%20%7C%2011-0078D6?logo=windows)
![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)
![Licence MIT](https://img.shields.io/badge/licence-MIT-green)

![Réglages](docs/settings.png)

</div>

## Pourquoi

MCHOSE HUB est une application Electron qui charge son interface depuis Internet et laisse
une dizaine de processus tourner en permanence, alors que tous les réglages de la souris sont
stockés **dans la souris elle-même**. MCHOSE Lite les écrit directement et ne garde en fond
qu'une petite icône de batterie, facultative.

| | MCHOSE HUB | MCHOSE Lite |
|---|---|---|
| En fond | ~110 Mo, ~10 processus | **~2 Mo** (icône de batterie, facultative) |
| Fenêtre ouverte | (inclus ci-dessus) | ~7 Mo (rendu CPU, sans GPU) |
| Connexion Internet | requise pour l'interface | jamais |
| Exécutable | ~100 Mo installés | ~4 Mo, aucune installation |

## Fonctionnalités

- **Performance** : polling rate 125 Hz → 8 kHz (selon le récepteur), mode de performance
  *Standard / Rapide / Ultra*.
- **DPI** : 1 à 6 niveaux, niveau actif, valeurs X/Y séparées en option (jusqu'à 26 000 DPI,
  42 000 sur les modèles Ultra).
- **Capteur** : distance de décollage (LOD 1 / 2 mm), ripple control, angle snapping,
  motion sync, anti-rebond (debounce) des clics.
- **Économie d'énergie** : délai de mise en veille ou veille désactivée.
- **Profils embarqués** 1 à 3, commutables depuis l'app ou en ligne de commande.
- **Seuls les réglages modifiés** sont écrits dans la souris.
- **Souris en veille détectée** : l'app ne propose jamais d'écrire une configuration vide.

### Batterie dans la zone de notification

<img src="docs/tray-widget.png" alt="Widget batterie" align="right" width="300">

- Icône MCHOSE avec jauge de batterie colorée (grisée si la souris dort).
- Info-bulle « MCHOSE L7 Pro — 82 % ».
- Clic gauche : widget avec niveau, état de charge, connexion et accès aux réglages.
- Clic droit : *Ouvrir les réglages, Actualiser, Lancer au démarrage, Quitter*.
- Alerte unique sous 15 %.
- Batterie relue toutes les 60 s ; la souris n'est ouverte que pendant la lecture.

<br clear="right">

## Matériel pris en charge

| Appareil | USB | État |
|---|---|---|
| MCHOSE L7 Pro (récepteur 8K) | `5253:1020` | testé (firmware 5.12.5.0) |
| Autres L7 / A7 / M7 / K7 (VID `5253`) | | même protocole, non testé |

## Installation

1. Ferme **MCHOSE HUB** et retire-le du démarrage.
2. Récupère `mchose-lite.exe` (voir [Compilation](#compilation)) et place-le où tu veux.
3. Lance-le pour régler la souris. Active *Icône de batterie au démarrage* si tu veux la
   batterie dans la barre des tâches.

> Windows 11 range les nouvelles icônes dans le menu `^` de la barre des tâches :
> fais-la glisser dans la barre pour la garder visible.

## Utilisation en ligne de commande

```text
mchose-lite              ouvre la fenêtre de réglages
mchose-lite tray         icône de batterie (relancer ouvre le widget)
mchose-lite status       batterie, firmware et réglages actuels
mchose-lite profile N    active le profil embarqué N (1-3)
```

```text
> mchose-lite status
MCHOSE L7 Pro (2.4G)  firmware 5.12.5.0
batterie   : 82%
profil     : 1
DPI        : 400 800 [1000] 3200 6400 26000
polling    : 1000 Hz
LOD        : 2 mm
debounce   : 8 ms
veille     : 3 min
```

## Compilation

Prérequis : [Rust](https://rustup.rs) avec la toolchain **MSVC** (Visual Studio Build Tools).

```powershell
git clone https://github.com/Brokeos/mchose-lite
cd mchose-lite
cargo build --release
# → target\release\mchose-lite.exe
```

## Protocole

Le protocole a été reconstitué à partir du driver web officiel, puis vérifié sur le matériel :

- feature reports `0x11` (20 octets) et `0x12` (64 octets) sur la collection HID vendeur
  `usage page 0xFF01` ;
- tous les octets après l'identifiant de report sont inversés (`XOR 0xFF`) ;
- chaque commande est acquittée par un input report `0x13` : `E5 <commande>` ;
- `0x12 0x67` lit le profil actif, `0x12 0x40` écrit les DPI, `0x11 0x41` le polling rate,
  `0x11 0x42` le capteur, `0x11 0x43` l'anti-rebond, `0x11 0x0A` la veille,
  `0x11 0x58` le profil, `0x11 0x06` la batterie.

Le détail est dans [`src/protocol.rs`](src/protocol.rs).

## Limites

- Pas de remappage des boutons, de macros ni d'éclairage.
- Pas de mise à jour de firmware : utilise l'outil officiel pour ça.
- Windows uniquement.

## Mentions légales

Projet indépendant, non affilié à MCHOSE (迈从科技). Les marques et l'icône de l'application restent
la propriété de leurs titulaires ; le protocole a été documenté à des fins d'interopérabilité.
**Ayants droit : pour toute demande de retrait (icône, marque, documentation), ouvrez une
issue et l'élément sera retiré rapidement.** Détails dans [LEGAL.md](LEGAL.md).

Logiciel fourni tel quel, sans garantie.

## Licence

[MIT](LICENSE)
