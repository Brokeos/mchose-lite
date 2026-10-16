# Mentions légales

## Non-affiliation

Ce projet est indépendant. Il n'est ni développé, ni approuvé, ni soutenu par MCHOSE (迈从科技). « MCHOSE », « MCHOSE HUB », ainsi que les logos et icônes associés, sont des marques ou des œuvres protégées appartenant à leurs titulaires.

## Rétro-ingénierie

Le protocole HID décrit dans `src/protocol.rs` et dans le README a été reconstitué dans un seul but : **l'interopérabilité**, c'est-à-dire permettre à un programme indépendant de régler la souris de l'utilisateur sans le logiciel officiel.

- L'analyse est **statique** : lecture du code JavaScript du driver web officiel, tel qu'il est servi publiquement à tout navigateur, puis vérification par des lectures sur la souris de l'auteur.
- Aucun identifiant, jeton, clé ni fichier d'utilisateur n'a été lu, déchiffré ou envoyé pour produire ce travail.
- Le code de MCHOSE HUB et de son driver web n'est **pas redistribué** dans ce dépôt. Le code de ce projet est une réécriture indépendante qui décrit des formats de messages, pas des copies de code.
- Aucune protection technique (chiffrement, DRM, signature de firmware) n'est contournée. Le firmware de la souris n'est jamais modifié.

Dans l'Union européenne, la décomposition d'un programme à des fins d'interopérabilité est encadrée par l'article 6 de la directive 2009/24/CE (en France, article L122-6-1 IV du Code de la propriété intellectuelle). Ce texte n'est cité qu'à titre d'information et ne constitue pas un avis juridique.

## Contenus tiers inclus

- `assets/icon.ico` : icône de MCHOSE HUB, reprise de l'application officielle pour que l'application reste reconnaissable. Elle reste la propriété de MCHOSE et n'est pas couverte par la licence du code.

**Si vous êtes titulaire de droits sur l'un de ces éléments (icône, marque, documentation du protocole) et souhaitez son retrait, ouvrez une issue sur le dépôt ou contactez l'auteur : il sera retiré rapidement.**

## Usage et responsabilité

- Usage personnel, sans but commercial. Aucune donnée n'est collectée ni envoyée : le logiciel ne communique qu'avec le matériel branché sur la machine, jamais avec Internet.
- Modifier les réglages de la souris avec un logiciel tiers se fait à vos risques ; l'application n'écrit que les réglages que vous changez.
- Le logiciel est fourni « tel quel », sans garantie d'aucune sorte. L'auteur n'est pas responsable des dommages matériels, pertes de données ou problèmes de garantie liés à son utilisation.
