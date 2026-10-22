# Éclairage du MCHOSE MagDock

Implémentation indépendante vérifiée le 22 octobre 2026 sur le dock USB
`3837:1012`, collection HID `FF00:0001`. Le dock est ouvert séparément de la
souris ; une souris absente ou en veille n'empêche pas son réglage.

## Transport

Le dock utilise des output/input reports sans numéro. L'octet d'identifiant nul
est ajouté pour les écritures hidapi. Les données ne sont pas inversées, à la
différence du protocole de la souris.

```text
AA <commande> <type> <séquence> <total> <longueur> <données...>
```

Le type vaut `00` pour une requête et `02` pour une réponse. La lecture de l'état
utilise la commande `07`, sans données. Sa réponse contient au moins 30 octets.
Les réponses d'autres commandes et les notifications sont ignorées ; les réponses
tronquées sont rejetées. Une attente bornée évite de bloquer indéfiniment la fenêtre.

## État et écriture

La commande `27` reçoit dix octets de configuration :

| Position | Champ | Valeurs |
|---|---|---|
| 0 | Éclairage activé | 0 / 1 |
| 1 | Effet | 0 fixe, 1 respiration, 2 scintillement, 3 cycle, 4 vague, 5 musique |
| 2 | Paramètre de nombre d'effets | Préservé depuis l'état du dock ; 6 sur le matériel testé |
| 3 | Vitesse | 0 à 4 ; 0 = le plus rapide |
| 4 | Luminosité | 0 à 4 ; cinq niveaux, extinction séparée |
| 5 | Synchronisation musicale | Préservée ; désactivée lors du choix d'un effet autonome |
| 6–8 | Couleur | Rouge, vert, bleu, chacun entre 0 et 255 |
| 9 | Direction | Préservée depuis l'octet 29 de la réponse d'état |

Les positions 0 à 8 correspondent à celles des données de la réponse `07`.
Les informations de veille et de version ainsi que les octets réservés de cette
réponse ne sont pas écrits par la commande d'éclairage.

L'interface propose les cinq effets autonomes. Seuls fixe et respiration exposent
une couleur personnalisée. Le mode musical existant peut être lu et préservé,
mais l'application n'envoie aucun flux audio et ne propose pas de l'activer.

Après chaque écriture, le programme relit l'état et compare les dix paramètres.
Un échec ou une déconnexion conserve les modifications dans l'interface avec un
message d'erreur ; aucun succès n'est annoncé sans confirmation du dock.

## Vérifications

- Tests unitaires sur une réponse réelle, les en-têtes, les longueurs, les valeurs
  invalides et le format de la commande RGB.
- Test matériel des cinq effets, des couleurs rouge / vert / bleu, des cinq
  niveaux de luminosité, des cinq vitesses et de l'extinction.
- Relecture via un nouveau handle HID pour vérifier que les valeurs sont
  conservées hors du handle qui a écrit la configuration.
- Restauration et relecture de l'état initial après le test.

Ces vérifications confirment les réglages rapportés par le matériel. Elles ne
mesurent pas la lumière émise et ne constituent pas un essai après coupure USB.

## Source primaire

[Pilote web officiel MCHOSE](https://www.mchose.com.cn/), fichier public
[`purify.es-CxLZ1Htw.js`](https://www.mchose.com.cn/assets/purify.es-CxLZ1Htw.js)
consulté le 22/10/2026 : format des reports MagDock, commandes de lecture et
d'éclairage, effets et plages des réglages. Le code du pilote n'est pas redistribué.
