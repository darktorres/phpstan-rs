<?php
            enum Mode {
                case Compatible;
                case Lenient;
            }
            interface BackedEnum {}
            interface UnitEnum {}
            /** @param list<mixed> $data */
            function f(Mode $mode, array $data): void {
                foreach ($data as $value) {
                    if ($value instanceof BackedEnum) {
                        if (Mode::Compatible !== $mode) {
                            continue;
                        }
                        $value = get_object_vars($value);
                    }
                    if ($value instanceof UnitEnum) {
                        if (Mode::Lenient === $mode) {
                            continue;
                        }
                        $value = get_object_vars($value);
                    }
                }
            }
        