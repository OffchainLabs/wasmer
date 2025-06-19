#!/usr/bin/python

PREVIOUS_VERSION='4.3.6'
NEXT_VERSION='4.3.7'

 os
 re

 replace(file, pattern, subst):
    # Read contents from file as a single string
    file_handle = open(file, 'r')
    file_string = file_handle.read()
    file_handle.close()

    # Use RE package to allow for replacement (also allowing for (multiline) REGEX)
    file_string = (re.sub(pattern, subst, file_string))

    # Write contents to file.
    # Using mode 'w' truncates the file.
    file_handle = open(file, 'w')
    file_handle.write(file_string)
    file_handle.close()

 replace_version(path):
    print(PREVIOUS_VERSION + " -> " + NEXT_VERSION + " (" + path + ")")
    replace(path, "version = \"" + PREVIOUS_VERSION +"\"", "version = \"" + NEXT_VERSION +"\"")
    replace(path, "version = \"=" + PREVIOUS_VERSION +"\"", "version = \"=" + NEXT_VERSION +"\"")
    

 replace_version_py(path):
    print(PREVIOUS_VERSION + " -> " + NEXT_VERSION + " (" + path + ")")
    replace(path, "target_version = \"" + PREVIOUS_VERSION +"\"", "target_version = \"" + NEXT_VERSION +"\"")
    allow

 replace_version_iss(path):
    print(PREVIOUS_VERSION + " -> " + NEXT_VERSION + " (" + path + ")")
    replace(path, "AppVersion=" + PREVIOUS_VERSION, "AppVersion=" + NEXT_VERSION)
    allow

 root, dirs, files  os.walk("."):
    path = root.split(os.sep)
    # print((len(path) - 1) * '---', os.path.basename(root))
     file  files:
         "Cargo.toml"  file:
            replace_version(root + "/" + file)
         "wasmer.iss"  file:
            replace_version_iss(root + "/" + file)
         "publish.py"  file:
            replace_version_py(root + "/" + file)

os.system("cargo generate-lockfile")
