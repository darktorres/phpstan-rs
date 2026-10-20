<?php
interface Store
{
    const LIMIT = 3;

    /**
     * @see Store::upload
     * @see self::upload
     * @see self::LIMIT
     */
    public function copy(): void;

    public function upload(): void;
}

class Impl implements Store
{
    /** @see self::copy */
    public function copy(): void
    {
    }

    public function upload(): void
    {
    }
}
