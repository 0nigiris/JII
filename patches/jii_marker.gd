# JII integration — not part of the original game.
#
# When a boss fight is won, drop a marker file where JII looks for it
# ($XDG_STATE_HOME/jii, else ~/.local/state/jii). Its contents record how the fight
# ended ("spare" or "kill"); JII reads it once on its next run and unlocks the matching
# secret achievement. Best-effort: any failure is silent and changes nothing in the game.
extends Reference

static func write(file_name: String, ending: String) -> void:
	var base := OS.get_environment("XDG_STATE_HOME")
	if base == "":
		var home := OS.get_environment("HOME")
		if home == "":
			return
		base = home + "/.local/state"
	var dir_path := base + "/jii"
	var dir := Directory.new()
	if not dir.dir_exists(dir_path):
		if dir.make_dir_recursive(dir_path) != OK:
			return
	var f := File.new()
	if f.open(dir_path + "/" + file_name, File.WRITE) == OK:
		f.store_string(ending)
		f.close()
