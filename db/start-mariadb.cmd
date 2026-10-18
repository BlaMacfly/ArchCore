@echo off
rem Demarre l'instance MariaDB locale d'ArchCore (port 3306, root/archcore)
"C:\Program Files\MariaDB 13.0\bin\mariadbd.exe" --defaults-file="%~dp0data\my.ini" --console
