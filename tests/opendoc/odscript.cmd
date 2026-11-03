/* ODSHELL's Document/Run script: scripting the document from REXX */
shell = .ScriptableApp~new~connect('ODSHELL')
shell~settext('Text set by a REXX script')
shell~play
return shell~gettext
