; L'installeur tourne sans droits admin (installMode currentUser) : dans un
; dossier protégé comme `Program Files`, la copie des fichiers échoue. Sur la
; page du dossier, Next reste grisé et la raison remplace le texte d'intro.
; Appelé à chaque frappe dans le champ, d'où un test léger.
Var InstDirIntroText
Var InstDirDefault
Var InstDirPage

Function .onVerifyInstDir
  Push $0
  Push $1
  Push $2
  Push $3

  ; Le champ du dossier (1019) identifie la page, $3 est son dialog.
  ; Sans fenêtre (mode silencieux), FindWindow prendrait n'importe quel dialog.
  StrCpy $2 0
  ${If} $HWNDPARENT <> 0
    FindWindow $3 "#32770" "" $HWNDPARENT
    GetDlgItem $2 $3 1019
  ${EndIf}

  ; Chaque affichage de la page crée un nouveau dialog : y revenir (Back puis
  ; Next) remet le dossier de son premier affichage.
  ${If} $2 <> 0
  ${AndIf} $3 <> $InstDirPage
    StrCpy $0 $InstDirPage
    StrCpy $InstDirPage $3
    ${If} $0 == ""
      StrCpy $InstDirDefault $INSTDIR
    ${ElseIf} $INSTDIR != $InstDirDefault
      StrCpy $INSTDIR $InstDirDefault
      SendMessage $2 ${WM_SETTEXT} 0 "STR:$INSTDIR"
    ${EndIf}
  ${EndIf}

  ; Premier dossier existant du chemin : l'installeur y créera le reste.
  StrCpy $0 $INSTDIR
  ${DoUntil} ${FileExists} "$0\*.*"
    ${GetParent} $0 $0
    ${IfThen} $0 == "" ${|} ${ExitDo} ${|}
  ${Loop}
  StrCpy $1 0
  ${If} $0 != ""
    ClearErrors
    CreateDirectory "$0\.unalia-write-test"
    ${IfNot} ${Errors}
      RMDir "$0\.unalia-write-test"
      StrCpy $1 1
    ${EndIf}
  ${EndIf}

  ; 1006 : texte d'intro de la page.
  ${If} $2 <> 0
    GetDlgItem $2 $3 1006
    ${If} $InstDirIntroText == ""
      System::Call 'user32::GetWindowText(p r2, t .s, i ${NSIS_MAX_STRLEN})'
      Pop $InstDirIntroText
    ${EndIf}
    ${If} $1 = 1
      SendMessage $2 ${WM_SETTEXT} 0 "STR:$InstDirIntroText"
    ${Else}
      SendMessage $2 ${WM_SETTEXT} 0 "STR:Unalia Mod Manager can't be installed in this folder: it needs administrator rights, and this installer runs without them. Pick a folder in your user profile, such as the default one."
    ${EndIf}
  ${EndIf}

  Pop $3
  Pop $2
  ${If} $1 = 1
    Pop $1
    Pop $0
  ${Else}
    Pop $1
    Pop $0
    Abort
  ${EndIf}
FunctionEnd

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
