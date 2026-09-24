<?php
            trait Common {
                public function check(): void {
                    if (self::FLAG) {
                        echo 1;
                    }
                }
            }
            class HostTrue {
                use Common;
                const FLAG = true;
            }
            class HostFalse {
                use Common;
                const FLAG = false;
            }
        