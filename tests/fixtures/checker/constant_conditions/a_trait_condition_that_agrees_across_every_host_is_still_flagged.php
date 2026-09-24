<?php
            trait Common {
                public function check(): void {
                    if (self::FLAG) {
                        echo 1;
                    }
                }
            }
            class HostA {
                use Common;
                const FLAG = true;
            }
            class HostB {
                use Common;
                const FLAG = true;
            }
        