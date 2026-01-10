#!/bin/bash
# Helper script to run benchmarks and show report locations

cd "$(dirname "$0")/.."

echo "Running benchmarks..."
cargo bench

echo ""
echo "🎯 Benchmark HTML Reports Generated!"
echo ""
echo "📊 Main Report:"
echo "   file://$(pwd)/target/criterion/report/index.html"
echo ""
echo "Individual Reports:"
echo "   Simple:  file://$(pwd)/target/criterion/parse_simple_idl/report/index.html"
echo "   Complex: file://$(pwd)/target/criterion/parse_complex_idl/report/index.html"
echo "   Large:   file://$(pwd)/target/criterion/parse_large_idl/report/index.html"
echo ""
echo "💡 Tip: Open these URLs in your browser to view detailed statistics and charts"
