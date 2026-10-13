BASIC SYNTH - build it with no installs
1. Make a free account at github.com, click "+" > New repository (any name, Public or Private).
2. On the new repo page click "uploading an existing file" and drag in EVERYTHING from this folder
   (including the hidden .github and .cargo folders - if you can't see them, upload the ZIP's contents via GitHub Desktop
   or tell Claude and we'll find another way).
3. Go to the "Actions" tab. The build starts automatically (~5-10 min). 
4. Click the finished run > scroll to "Artifacts" > download the one for your computer.
5. Windows: put BasicSynth .vst3 in C:\Program Files\Common Files\VST3\
   Mac: put it in /Library/Audio/Plug-Ins/VST3/ then run in Terminal:  xattr -cr /Library/Audio/Plug-Ins/VST3/basic_synth.vst3
6. Ableton: Preferences > Plug-Ins > rescan. Find "Basic Synth" under Plug-Ins > Instruments. Drop on a MIDI track.
