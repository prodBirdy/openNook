import { CompactWrapper } from './CompactWrapper';
import { IconPhoto } from '@tabler/icons-react';
import { useFileTrayStore } from '../../stores/useFileTrayStore';

interface CompactFilesProps {
    isHovered: boolean;
    baseNotchWidth: number;
    contentOpacity: number;
}

export function CompactFiles({
    isHovered,
    baseNotchWidth,
    contentOpacity
}: CompactFilesProps) {
    const files = useFileTrayStore(state => state.files);
    return (
        <CompactWrapper
            id="files-content"
            className="island-content files-content"
            baseNotchWidth={baseNotchWidth}
            isHovered={isHovered}
            contentOpacity={contentOpacity}
            left={
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                    <IconPhoto size={20} color="white" stroke={1.5} />
                </div>
            }
            right={
                <div style={{ display: 'flex', alignItems: 'center', color: 'white' }}>
                    {files.length}
                </div>
            }
        />
    );
}
