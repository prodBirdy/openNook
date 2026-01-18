import { CompactWrapper } from './CompactWrapper';
import { IconPhoto } from '@tabler/icons-react';
import { useFileTrayStore } from '../../stores/useFileTrayStore';

interface CompactFilesProps {
    isHovered: boolean;
    baseNotchWidth: number;
    contentOpacity?: number;
}

export function CompactFiles({
    isHovered,
    baseNotchWidth
}: CompactFilesProps) {
    const files = useFileTrayStore(state => state.files);
    return (
        <CompactWrapper
            id="files-content"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            left={
                <div className="flex items-center gap-2">
                    <IconPhoto size={20} color="white" stroke={1.5} />
                </div>
            }
            right={
                <div className="flex items-center text-white">
                    {files.length}
                </div>
            }
        />
    );
}
