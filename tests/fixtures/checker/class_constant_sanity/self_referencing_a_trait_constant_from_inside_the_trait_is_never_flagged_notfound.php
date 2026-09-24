<?php
            trait Walker {
                private const string STOP = "stop";
                public function walk(): string {
                    return self::STOP;
                }
            }
            class HostA {
                use Walker;
            }
            class HostB {
                use Walker;
            }
        