; The installer runs without admin rights (installMode currentUser): in a
; protected folder like `Program Files`, copying the files fails. On the
; folder page, Next stays grayed out and the reason replaces the intro text.
; Called at each keystroke in the field, hence a light check.
Var InstDirIntroText
Var InstDirDefault
Var InstDirPage

Function .onVerifyInstDir
  Push $0
  Push $1
  Push $2
  Push $3

  ; The folder field (1019) identifies the page, $3 is its dialog.
  ; Without a window (silent mode), FindWindow would grab any dialog.
  StrCpy $2 0
  ${If} $HWNDPARENT <> 0
    FindWindow $3 "#32770" "" $HWNDPARENT
    GetDlgItem $2 $3 1019
  ${EndIf}

  ; Each display of the page creates a new dialog: going back to it (Back
  ; then Next) restores the folder of its first display.
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

  ; First existing folder of the path: the installer will create the rest.
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

  ; 1006: intro text of the page.
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

; Uninstall with the "delete the data" box ticked: the mods leave the games
; before the database that lists them disappears. Box unticked (or an
; update): the mods stay, a reinstall finds them again.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    ; Also done right after by the uninstaller, but the open app would
    ; write to the database while it is being emptied.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    DetailPrint "Removing mods from your games..."
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --remove-all-mods'
  ${EndIf}
!macroend
