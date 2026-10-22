# K7 Ultra : fréquence USB et 2.4G

Vérification du 22 octobre 2026 sur une MCHOSE K7 Ultra, firmware `5.28.5.0`,
connectée en 2.4G via le récepteur `5253:1020`.

## Cause

La réponse décodée à `0x12 0x67`, sans l'octet de commande, commence ainsi :

```text
00 20 50 00 B0 04 E2 04 E2 04 E2 04 E2 04 E2 04 01 02 08 03
```

| Position | Sens | Valeur observée |
|---|---|---|
| 0 | Profil | 0 |
| 1, bits 0–3 | Niveau DPI USB | 0 |
| 1, bits 4–7 | Indice de fréquence USB | 2 = 1 000 Hz |
| 2, bits 0–3 | Niveau DPI 2.4G | 0 |
| 2, bits 4–7 | Indice de fréquence 2.4G | 5 = 8 000 Hz |

L'ancien décodage inversait à la fois les connexions et les demi-octets : il lisait
un indice DPI sans-fil de 2 et un indice de fréquence de 0 (125 Hz). La réponse
L7 Pro utilisée par le test existant contient `22 22`, ce qui masque ces inversions.

Le parseur du pilote web officiel utilise des champs `bit4` en little-endian et
regroupe les deux octets avant de les décoder. Il faut tenir compte de ce
regroupement pour retrouver leur position sur le fil.

## Correction

- Décodage USB puis 2.4G, avec le DPI dans les bits bas et la fréquence dans les bits hauts.
- Commande de fréquence : `0x11 0x41 <indice USB> <indice 2.4G>` ; conservation de
  la fréquence de l'autre connexion.
- Relecture de la configuration après écriture ; erreur si la fréquence attendue
  n'est pas confirmée, sans annoncer un enregistrement réussi.
- Rejet des configurations nulles renvoyées pendant la veille : message explicite
  au lieu de présenter 125 Hz ou de confirmer à tort un changement vers 125 Hz.
- Reconnaissance des interfaces vendeur `0xFF01` pour la K7 Ultra filaire
  `3837:4150` et son récepteur dédié `3837:1001`, en plus du récepteur `5253:1020`.
- Priorité à l'interface filaire K7 lorsque le câble et le récepteur sont branchés.
- Limite de 1 000 Hz conservée pour le récepteur `1021`.

## Validation matérielle

Le test `hardware_k7_ultra_polling_rates` a appliqué puis relu successivement
125, 500, 1 000, 2 000, 4 000 et 8 000 Hz. À chaque étape, la configuration complète
a été comparée à celle attendue : seul l'indice de fréquence sans-fil a changé.
La configuration initiale, dont 8 000 Hz en sans-fil, a été restaurée et relue.

Cette validation porte sur l'enregistrement des réglages dans le matériel. Elle
ne mesure pas la cadence effective des événements USB. Le mode filaire K7 et le
récepteur `3837:1001` restent à vérifier physiquement.

## Sources primaires

- [Pilote web officiel MCHOSE](https://www.mchose.com.cn/), consulté le 22/10/2026 ;
  définitions du profil, de la commande `0x41`, des identifiants matériels et de la
  table des fréquences K7 Ultra dans le fichier public
  [purify.es-CxLZ1Htw.js](https://www.mchose.com.cn/assets/purify.es-CxLZ1Htw.js).
- [Guide officiel K7](https://support.mchose.store/hc/en-us/articles/50914154771476-K7-Tri-Mode-Gaming-Mouse-User-Guide) :
  configuration via USB ou 2.4G ; Bluetooth à fréquence fixe, sans accès au pilote.

Le code du pilote officiel n'est pas inclus dans ce dépôt.
