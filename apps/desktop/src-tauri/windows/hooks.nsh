; Désinstallation avec la case « supprimer les données » cochée : les mods
; quittent les jeux avant que la base qui les liste ne disparaisse. Case
; décochée (ou mise à jour) : les mods restent, une réinstallation les
; retrouve.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Fait aussi juste après par le désinstalleur, mais l'appli ouverte
    ; écrirait dans la base pendant qu'on la vide.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    DetailPrint "Removing mods from your games..."
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-all-mods'
  ${EndIf}
!macroend
