#!/bin/sh

export PYO3_CROSS_LIB_DIR="/usr/local/opt/python@3.9/Frameworks/Python.framework/Versions/3.9/lib"
cargo  lipo --release && cp target/universal/release/libclvm_tools_rs.a ../ios/Classes