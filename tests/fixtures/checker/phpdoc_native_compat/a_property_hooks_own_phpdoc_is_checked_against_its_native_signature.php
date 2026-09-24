<?php
            class C {
                public int $a {
                    /** @return string */
                    get => 1;
                    /** @param string $value */
                    set(int $value) { $this->a = $value; }
                }
                public int $b {
                    /**
                     * @param int<0, max> $value
                     * @param int $other
                     * @param-out int $value
                     */
                    set { $this->b = $value; }
                }
            }
        