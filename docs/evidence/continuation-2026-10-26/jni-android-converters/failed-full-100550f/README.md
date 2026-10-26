# Retained complete-validator attempt at 100550f

Terminal exit 1. ARM64 native build, ELF check and desktop native library build passed. JNI Java compilation failed because tools/validate_android.py used an explicit source list that omitted the new ConverterBoundarySmoke fixture referenced by BoundarySmoke. The separate component validator compiled all Java fixtures and passed 52 groups before/after R8; that did not make this full attempt pass. The validator source list and converter fixture-minification check are corrected separately; a fresh complete run is required.
