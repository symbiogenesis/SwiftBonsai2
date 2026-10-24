# Optional explicit source substitution; the verified upstream tree stays intact.
if(DEFINED PRISM_PATCHED_SAVER)
  if(NOT IS_ABSOLUTE "${PRISM_PATCHED_SAVER}" OR NOT EXISTS "${PRISM_PATCHED_SAVER}")
    message(FATAL_ERROR "PRISM_PATCHED_SAVER must name the external verified source")
  endif()
  file(SHA256 "${PRISM_SOURCE_DIR}/src/llama-model-saver.cpp" PRISM_SAVER_BEFORE)
  file(SHA256 "${PRISM_PATCHED_SAVER}" PRISM_SAVER_AFTER)
  if(NOT PRISM_SAVER_BEFORE STREQUAL "6dec1fc297dca89fb6933b638acf593a2e4c1ca02b69dafe00268651fa9ef96d" OR
     NOT PRISM_SAVER_AFTER STREQUAL "8b2deae2693ff2a664e3d53e82cea06f8166b4671eadd0ba4a957c2a5af53dee")
    message(FATAL_ERROR "Production saver source substitution differs from its explicit pin")
  endif()
  # Set source properties in the owning target's directory. All original source
  # files remain byte-identical to the archive and retain their cache identity.
  set_source_files_properties("${PRISM_SOURCE_DIR}/src/llama-model-saver.cpp"
    TARGET_DIRECTORY llama PROPERTIES HEADER_FILE_ONLY TRUE)
  target_sources(llama PRIVATE "${PRISM_PATCHED_SAVER}")
  target_include_directories(llama PRIVATE "${PRISM_SOURCE_DIR}/src")
endif()
