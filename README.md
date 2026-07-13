# UQAC_T3_MMO
UQAC Project 2026

objectif : construire la structure d'un MMO (permetant de tenir un nombre tres élevé de joueurs)

le projet est composé de plusieurs crates rust :
- orchestrator : gere la creation des serveurs et leur initialisation.
- gatekeeper : premier point de communication avec le client. (api web)
- broker : systeme de communication pub/sub central. 
- spatial_server : gere la taille des shard avec un quad-tree et place les players sur les bonnes shards.
- spatial_voronoi : gere la taille des shard avec un systeme voronoi et place les players sur les bonnes shards.
- client : jeux client permettant de se connecter a son compte est de controller un personnage se deplacent sur la map.
- server : DGS representant une shard. il applique les inputs des players qui gere et calcule la physique.
- aoi_server : guide les server pour qu'ils publie sur les bon chunks selon la position du joueur et son mode d'aoi.
- bot_swarm : outil permettant de simuler des joueurs au deplacement aléatoire
- tools : outil permettant d'inscrire automatiquement des player dans la BDD. 

## pour les serveurs :

avec le spacial serveur en Quad-tree :
```
docker compose -f docker-compose.yml --profile quadtree up -d
```

avec le spatial serveur en Voronoi :
```
docker compose -f docker-compose.yml --profile voronoi up -d
```

et pour tout éteindre : 
```
docker compose down
```

## pour les clients :
```
docker compose -f docker-compose-client.yml
```
