<?php
// Prepended to every fixture the runtime oracle runs
// (`tests/runtime_oracle.rs`): records each `assertType()` call's
// expected type and the value it got, and each warning, notice and
// deprecation PHP emits, one JSON object per line in the file
// `PHPSTAN_RS_ORACLE_OUT` names; `PHPSTAN_RS_ORACLE_AUTOLOAD`, when set,
// is the runtime packages' autoloader. Written for every PHP version a
// fixture may run under, 7.0 up: no `void`, no nullable types.

namespace PHPStan\Testing {
    function assertType(string $expected, $value)
    {
        \PhpstanRsOracle::recordAssert($expected, $value);
        return $value;
    }
}

namespace {
    if (!function_exists('assertType')) {
        function assertType(string $expected, $value)
        {
            \PhpstanRsOracle::recordAssert($expected, $value);
            return $value;
        }
    }

    final class PhpstanRsOracle
    {
        /** The most elements of one array that are recorded. */
        const MAX_ELEMENTS = 256;
        /** The deepest nesting that is recorded. */
        const MAX_DEPTH = 8;

        public static function recordAssert(string $expected, $value)
        {
            $call = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 3)[1] ?? [];
            self::write([
                'kind' => 'assert',
                'file' => $call['file'] ?? '',
                'line' => $call['line'] ?? 0,
                'expected' => $expected,
                'value' => self::encode($value, 0),
            ]);
        }

        public static function recordError(int $level, string $message, string $file, int $line): bool
        {
            if ((error_reporting() & $level) === 0) {
                return false;
            }
            self::write([
                'kind' => 'error',
                'level' => $level,
                'message' => $message,
                'file' => $file,
                'line' => $line,
            ]);
            return true;
        }

        /** @param mixed $value */
        private static function encode($value, int $depth): array
        {
            if ($depth > self::MAX_DEPTH) {
                return ['t' => 'unknown'];
            }
            if ($value === null) {
                return ['t' => 'null'];
            }
            if (is_bool($value)) {
                return ['t' => 'bool', 'v' => $value];
            }
            if (is_int($value)) {
                return ['t' => 'int', 'v' => $value];
            }
            if (is_float($value)) {
                return ['t' => 'float', 'bits' => bin2hex(pack('E', $value))];
            }
            if (is_string($value)) {
                return ['t' => 'string', 'hex' => bin2hex($value)];
            }
            if (is_array($value)) {
                if (count($value) > self::MAX_ELEMENTS) {
                    return ['t' => 'unknown'];
                }
                $items = [];
                foreach ($value as $key => $item) {
                    $items[] = [$key, self::encode($item, $depth + 1)];
                }
                return ['t' => 'array', 'items' => $items];
            }
            if (is_object($value)) {
                if ($value instanceof \UnitEnum) {
                    return ['t' => 'enum', 'class' => get_class($value), 'case' => $value->name];
                }
                $class = get_class($value);
                if (strpos($class, '@anonymous') !== false) {
                    return ['t' => 'object', 'class' => null];
                }
                return ['t' => 'object', 'class' => $class];
            }
            if (is_resource($value)) {
                return ['t' => 'resource', 'closed' => false];
            }
            if (gettype($value) === 'resource (closed)') {
                return ['t' => 'resource', 'closed' => true];
            }
            return ['t' => 'unknown'];
        }

        private static function write(array $record)
        {
            $out = getenv('PHPSTAN_RS_ORACLE_OUT');
            if ($out === false) {
                return;
            }
            // Before PHP 7.2, a string that is not UTF-8 is written as null
            // instead of with its bad bytes substituted.
            $flags = JSON_UNESCAPED_SLASHES | (\defined('JSON_INVALID_UTF8_SUBSTITUTE')
                ? JSON_INVALID_UTF8_SUBSTITUTE
                : JSON_PARTIAL_OUTPUT_ON_ERROR);
            $line = json_encode($record, $flags);
            if ($line !== false) {
                file_put_contents($out, $line . "\n", FILE_APPEND);
            }
        }
    }

    set_error_handler([PhpstanRsOracle::class, 'recordError']);

    $phpstanRsOracleAutoload = getenv('PHPSTAN_RS_ORACLE_AUTOLOAD');
    if ($phpstanRsOracleAutoload !== false && $phpstanRsOracleAutoload !== '') {
        require_once $phpstanRsOracleAutoload;
    }
    unset($phpstanRsOracleAutoload);
}
