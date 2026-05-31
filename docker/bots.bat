@echo off
echo Lancement des 5 bots de test...

FOR /L %%i IN (1,1,5) DO (
  echo Demarrage du bot_client_%%i...

  docker run -d ^
    --name bot_client_%%i ^
    --network game-network ^
    -e BOT_USER="user%%i" ^
    -e BOT_PASS="password%%i" ^
    my_bot_client:latest
)

echo.
echo Tous les bots ont ete lances avec succes !
pause