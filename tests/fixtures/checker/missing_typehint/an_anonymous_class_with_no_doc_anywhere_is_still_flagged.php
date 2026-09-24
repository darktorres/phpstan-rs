<?php
            interface UndocumentedComparator {
                public function compare(array $a): int;
            }
            function make(): UndocumentedComparator {
                return new class implements UndocumentedComparator {
                    public function compare(array $a): int {
                        return 0;
                    }
                };
            }
        